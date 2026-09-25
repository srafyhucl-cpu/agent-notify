// 骨架样式落在 styles/patterns.css（扫描微光骨架，保留旧类名兼容）。
import "../styles/patterns.css";

const DEFAULT_ROW_COUNT = 5;

export interface LoadingRowsProps {
  "aria-label": string;
  rows?: number;
}

export function LoadingRows({
  "aria-label": ariaLabel,
  rows = DEFAULT_ROW_COUNT,
}: LoadingRowsProps) {
  const safeRows = Math.max(1, Math.floor(rows));

  return (
    <div className="loading-rows" role="status" aria-label={ariaLabel}>
      {Array.from({ length: safeRows }, (_, index) => (
        <div className="loading-row" key={index} aria-hidden="true">
          <span className="loading-row-bar" />
          <span className="loading-row-bar loading-row-bar-short" />
          <span className="loading-row-bar" />
        </div>
      ))}
    </div>
  );
}
