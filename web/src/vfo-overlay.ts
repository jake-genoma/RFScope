export function vfoBand(center: number, rate: number, frequency: number, bandwidth: number) {
  if (!(rate > 0) || !(bandwidth > 0)) return null;
  const middle = 0.5 + (frequency - center) / rate;
  const half = bandwidth / rate / 2;
  if (middle + half < 0 || middle - half > 1) return null;
  return { left: Math.max(0, middle - half), right: Math.min(1, middle + half), middle };
}
