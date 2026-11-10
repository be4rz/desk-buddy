// buddy.look: `look(path)` shows the model an image file. Registered always,
// offered only while the model takes images (`modelToolNames`); the gate
// checks it as a read of its path.

import { readFileSync, statSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";
import { defineExtension, defineTool, type ToolRegistration } from "@earendil-works/pi-durable";
import { Type } from "typebox";
import type { Init } from "../../protocol.ts";

/** What providers take inline: the base64 of one image stays under 5 MB. */
export const MAX_IMAGE_BYTES = Math.floor(4.5 * 1024 * 1024 * 0.75);

/** The image type from the file's first bytes (not its name). */
export function imageType(b: Uint8Array): string | undefined {
  if (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) return "image/png";
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return "image/jpeg";
  if (b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x38) return "image/gif";
  if (b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50) return "image/webp";
  return undefined;
}

export function createLook(init: Init) {
  const look = defineTool({
    name: "look",
    description: "Look at an image file (png, jpeg, gif or webp, up to about 3 MB): you see the picture itself.",
    parameters: Type.Object({ path: Type.String({ description: "The image's path, relative to the working folder or absolute" }) }),
    replay: "safe",
    execute: async (args) => {
      const raw = String(args.path ?? "");
      const path = isAbsolute(raw) ? raw : resolve(init.workDir, raw);
      const fail = (t: string) => ({ output: [{ type: "text" as const, text: t }], isError: true });
      let size: number;
      try {
        const st = statSync(path);
        if (!st.isFile()) return fail(`${raw} isn't a file.`);
        size = st.size;
      } catch {
        return fail(`There's no file at ${raw}.`);
      }
      if (size > MAX_IMAGE_BYTES) return fail(`${raw} is ${(size / 1024 / 1024).toFixed(1)} MB; look takes images up to about 3 MB.`);
      const bytes = readFileSync(path);
      const mimeType = imageType(bytes);
      if (!mimeType) return fail(`${raw} isn't a png, jpeg, gif or webp image.`);
      return { output: [{ type: "image" as const, data: bytes.toString("base64"), mimeType }] };
    },
  }) as unknown as ToolRegistration;
  return defineExtension({ name: "buddy.look", tools: [look] });
}
