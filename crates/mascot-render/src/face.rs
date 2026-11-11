//! Face patches (design §3.4, D16): eyes and mouth drawn over the body at the frame's
//! top-left positions, whenever the pack's face layers cover the clip and direction.
//! Clips or directions the face doesn't cover keep the body's baked default face.

use std::collections::{BTreeMap, HashMap};

use mascot_core::pack::{FaceMeta, Ori, SetFace};
use mascot_core::{Dir, PackMeta};

use crate::{blit, read_png, Canvas, RenderError, Sprite};

/// `patches[(slot, state, dir)]`, `pos[(clip, dir)][slot][frame]` (top-left on the body frame);
/// `meta` says which dir's patches a clip draws (face v2 `patch_dirs`) and holds the set clips' faces.
pub(crate) struct FaceArt {
    pub patches: HashMap<(String, String, Dir), Sprite>,
    pub pos: HashMap<(String, Dir), BTreeMap<String, Vec<[i32; 2]>>>,
    pub meta: FaceMeta,
}

/// Decode the pack's face patches; `None` when the pack has no face source.
pub(crate) fn load(pack: &PackMeta) -> Result<Option<FaceArt>, RenderError> {
    let Some(face) = &pack.face else { return Ok(None) };
    let mut art = FaceArt { patches: HashMap::new(), pos: HashMap::new(), meta: face.clone() };
    for (slot, states) in &face.layers {
        for (state, dirs) in states {
            for (dir, layer) in dirs {
                let (w, h, rgba) = read_png(&layer.file)?;
                if w != layer.w as usize || h != layer.h as usize {
                    return Err(RenderError::Size {
                        path: layer.file.clone(),
                        msg: format!("{w}x{h}, face.json says {}x{}", layer.w, layer.h),
                    });
                }
                art.patches.insert((slot.clone(), state.clone(), *dir), Sprite::crop(&rgba, w, 0, w, h));
            }
        }
    }
    for (clip, dirs) in &face.clips {
        for (dir, slots) in dirs {
            art.pos.insert((clip.clone(), *dir), slots.clone());
        }
    }
    Ok(Some(art))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(face: &FaceArt, clip: &str, dir: Dir, frame: usize, eyes: &str, mouth: &str, out: &mut Canvas, ox: i32, oy: i32) {
    let Some(pos) = face.pos.get(&(clip.to_string(), dir)) else { return };
    // positions are the body dir's; the sprite is the head's (a listen head turned to the camera)
    draw_slots(face, pos, face.meta.patch_dir(clip, dir), frame, eyes, mouth, out, ox, oy);
}

/// A set clip's face (`set_clips`, p3): the `patch` dir's sprites at positions on the set frame,
/// whose top-left is (`ox`, `oy`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_set(face: &FaceArt, clip: &str, ori: Ori, frame: usize, eyes: &str, mouth: &str, out: &mut Canvas, ox: i32, oy: i32) {
    let Some(SetFace { patch, slots }) = face.meta.set_face(clip, ori) else { return };
    draw_slots(face, slots, *patch, frame, eyes, mouth, out, ox, oy);
}

#[allow(clippy::too_many_arguments)]
fn draw_slots(face: &FaceArt, pos: &BTreeMap<String, Vec<[i32; 2]>>, patch_dir: Dir, frame: usize, eyes: &str, mouth: &str, out: &mut Canvas, ox: i32, oy: i32) {
    for (slot, state) in [("eyes", eyes), ("mouth", mouth)] {
        let Some(p) = pos.get(slot).and_then(|v| v.get(frame)) else { continue };
        let Some(s) = face.patches.get(&(slot.to_string(), state.to_string(), patch_dir)) else { continue };
        blit(out, s, ox + p[0], oy + p[1]);
    }
}
