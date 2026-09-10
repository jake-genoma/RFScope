import { expect, test } from "vitest";
import { vfoBand } from "./vfo-overlay";
test("VFO overlay maps RF bandwidth to the displayed capture", () => {
  expect(vfoBand(100000000, 2000000, 100000000, 10000)).toEqual({ left: 0.4975, right: 0.5025, middle: 0.5 });
  expect(vfoBand(100000000, 2000000, 102000000, 10000)).toBeNull();
  expect(vfoBand(100000000, 0, 100000000, 10000)).toBeNull();
  expect(vfoBand(100000000, 2000000, 101000000, 10000)?.right).toBe(1);
});
