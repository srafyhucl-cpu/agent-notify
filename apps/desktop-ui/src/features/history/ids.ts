/**
 * 历史详情面板的稳定 DOM id。
 *
 * 列表侧（HistoryTable）用 `aria-controls` 指向它，详情侧（HistoryDetail）把它挂在
 * 各自状态的 `<section>` 上——两处共用同一份来源，避免各写一遍字符串。
 */
export const HISTORY_DETAIL_ID = "history-detail";
