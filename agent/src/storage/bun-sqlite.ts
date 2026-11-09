// SqliteDatabase facade over bun:sqlite, modelled on chord's node:sqlite adapter (MIT), which Bun cannot load.
import { Database, type Statement } from "bun:sqlite";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { SqliteDatabase, SqliteExecutor, SqliteValue } from "@earendil-works/pi-durable/storage/sqlite";

class Executor implements SqliteExecutor {
  constructor(protected db: Database, protected statements: Map<string, Statement>, private guard: () => void = () => {}) {}
  protected op<T>(fn: () => T): Promise<T> {
    this.guard();
    return Promise.resolve(fn());
  }
  private stmt(sql: string) {
    let s = this.statements.get(sql);
    if (!s) this.statements.set(sql, (s = this.db.prepare(sql)));
    return s;
  }
  exec(sql: string) { return this.op(() => { this.db.exec(sql); }); }
  run(sql: string, ...p: SqliteValue[]) { return this.op(() => { this.stmt(sql).run(...(p as any)); }); }
  runCounted(sql: string, ...p: SqliteValue[]) { return this.op(() => Number(this.stmt(sql).run(...(p as any)).changes)); }
  // bun:sqlite returns null for no row; the facade expects undefined.
  get<T extends object>(sql: string, ...p: SqliteValue[]) { return this.op(() => (this.stmt(sql).get(...(p as any)) ?? undefined) as T | undefined); }
  all<T extends object>(sql: string, ...p: SqliteValue[]) { return this.op(() => this.stmt(sql).all(...(p as any)) as T[]); }
}

export class BunSqliteDatabase extends Executor implements SqliteDatabase {
  private tail: Promise<unknown> = Promise.resolve();
  private closed = false;
  constructor(db: Database) { super(db, new Map()); }
  // Every operation runs in call order behind any open transaction.
  protected override op<T>(fn: () => T): Promise<T> {
    const next = this.tail.then(fn);
    this.tail = next.catch(() => {});
    return next;
  }
  transaction<T>(callback: (tx: SqliteExecutor) => Promise<T>): Promise<T> {
    const next = this.tail.then(async () => {
      this.db.exec("BEGIN IMMEDIATE");
      const scope = { active: true };
      const tx = new Executor(this.db, this.statements, () => { if (!scope.active) throw new Error("SQLite transaction handle is no longer active"); });
      try {
        const result = await callback(tx);
        scope.active = false;
        this.db.exec("COMMIT");
        return result;
      } catch (error) {
        scope.active = false;
        this.db.exec("ROLLBACK");
        throw error;
      }
    });
    this.tail = next.catch(() => {});
    return next;
  }
  close() {
    return this.op(() => {
      if (this.closed) return;
      this.closed = true;
      this.statements.clear();
      try { this.db.exec("PRAGMA wal_checkpoint(TRUNCATE)"); } finally { this.db.close(); }
    });
  }
}

export function openBunSqliteDatabase(path: string) {
  if (path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
  const db = new Database(path, { create: true, strict: false });
  db.exec("PRAGMA busy_timeout = 5000");
  db.exec("PRAGMA journal_mode = WAL");
  db.exec("PRAGMA synchronous = NORMAL");
  db.exec("PRAGMA wal_autocheckpoint = 1000");
  return new BunSqliteDatabase(db);
}
