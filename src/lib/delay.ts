/**
 * 确保异步操作至少持续 minMs 毫秒，
 * 用于按钮 loading 状态的视觉反馈（避免太快看不到变化）。
 *
 * @example
 * const data = await withMinDelay(fetchData(), 1000);
 */
export function withMinDelay<T>(promise: Promise<T>, minMs = 1000): Promise<T> {
  const delay = new Promise<void>((r) => setTimeout(r, minMs));
  return Promise.all([promise, delay]).then(([result]) => result);
}
