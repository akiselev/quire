import { readFile } from "node:fs/promises";

export interface Point {
  readonly x: number;
  y?: number;
}

enum Color { Red = 1, Green }

type Pair<T> = [T, T];

export abstract class Shape<T extends Point = Point> implements Iterable<T> {
  private static count = 0;
  constructor(public readonly origin: T) {
    super();
  }
  abstract area(): number;
  *[Symbol.iterator](): Iterator<T> {
    yield this.origin;
  }
}

export async function load(path: string, opts?: { strict: boolean }): Promise<string> {
  const text = await readFile(path, "utf8");
  // A comment
  return opts?.strict ? text.trim() : `${text}\n`;
}

const re = /ab+c/gi;
let n: unknown = null as unknown as number;
