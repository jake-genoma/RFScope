import { expect, test } from "vitest";
import { AudioQueue } from "./audio-queue";
test("audio queue remains bounded and reports discontinuities", () => {
  const queue = new AudioQueue();
  queue.push(new Float32Array(2880).fill(0.5), 1n, 1n);
  expect(queue.buffered).toBe(2880); expect(queue.sample(1)).toBeCloseTo(0.5);
  queue.push(new Float32Array([0.1]), 1n, 4n);
  expect(queue.discontinuities).toBe(1); expect(queue.buffered).toBe(1);
  queue.push(new Float32Array(10000).fill(0.25), 2n, 1n);
  expect(queue.overruns).toBe(1); expect(queue.data.length).toBe(7680);
});
