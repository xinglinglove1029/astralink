export function paginate<T>(items: readonly T[], requestedPage: number, requestedSize: number) {
  const pageSize = [20,50,100].includes(requestedSize) ? requestedSize : 20
  const total = items.length
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  const page = Math.min(pageCount, Math.max(1, Number.isFinite(requestedPage) ? Math.floor(requestedPage) : 1))
  const offset = (page-1)*pageSize
  return { page, pageSize, pageCount, total, from:total ? offset+1 : 0, to:Math.min(offset+pageSize,total), items:items.slice(offset,offset+pageSize) }
}
export function pageSelection(selected: readonly string[], visible: readonly string[]): string[] {
  const ids = new Set(visible)
  return [...new Set(selected)].filter(id=>ids.has(id))
}
