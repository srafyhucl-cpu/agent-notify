import { useState } from "react";

import type { OrcRoundRecordDto } from "../../bridge/types";

export interface RoundTimelineProps {
  /** 任务描述：第 1 轮的「本轮要求」即它。 */
  goal: string;
  /** 当前轮次（从 1 起）。 */
  currentRound: number;
  /** 当前轮要求（roundInput；旧任务没有轮次记录时兜底展示）。 */
  currentInput: string | null;
  /** 后端记录的轮次时间线（旧任务可能为空）。 */
  records: OrcRoundRecordDto[];
  /** 当前轮是否已结束（终态任务 = 已结束；用于「本轮进行中」提示）。 */
  roundFinished: boolean;
}

/** 单轮展示数据：要求（第 1 轮 = 任务描述）+ 结论摘要。 */
interface RoundView {
  round: number;
  input: string | null;
  summary: string | null;
}

/** 折叠堆叠预览的轮数上限：更多轮由「展开全部 N 轮」进入完整时间线。 */
const ROUND_STACK_PREVIEW_COUNT = 3;

/**
 * 合并后端记录与当前轮次：缺记录的旧轮（历史裁剪/旧任务）以空要求占位，不猜内容；
 * 第 1 轮的要求即任务描述；当前轮缺记录时回退 roundInput（旧任务兼容）。
 */
function buildRoundViews(
  goal: string,
  currentRound: number,
  currentInput: string | null,
  records: OrcRoundRecordDto[],
): RoundView[] {
  const views: RoundView[] = [];
  for (let round = 1; round <= currentRound; round += 1) {
    const record = records.find((item) => item.round === round);
    const fallback = round === currentRound ? currentInput : null;
    views.push({
      round,
      input: round === 1 ? goal : (record?.input ?? fallback ?? null),
      summary: record?.summary ?? null,
    });
  }
  return views;
}

/** 要求文案：未填写/未记录时明确说明，不显示空白。 */
function requirementText(view: RoundView): string {
  if (view.input) {
    return view.input;
  }
  return view.round === 1
    ? "（见任务描述）"
    : "（本轮未填写要求，由项目经理按上一轮结论继续）";
}

/**
 * 轮次时间线（任务描述块内）：默认叠放最近几轮卡片，点「展开全部 N 轮」看完整时间线。
 * 只有一轮时不渲染（任务描述本身就是第 1 轮要求）。
 */
export function RoundTimeline({
  goal,
  currentRound,
  currentInput,
  records,
  roundFinished,
}: RoundTimelineProps) {
  const [expanded, setExpanded] = useState(false);
  if (currentRound <= 1) {
    return null;
  }
  const views = buildRoundViews(goal, currentRound, currentInput, records);
  const preview = views.slice(-ROUND_STACK_PREVIEW_COUNT).reverse();

  if (expanded) {
    return (
      <div className="cluster-round-timeline">
        <div className="cluster-round-timeline-head">
          <span className="cluster-round-timeline-title">迭代时间线</span>
          <button
            className="cluster-round-toggle"
            type="button"
            onClick={() => setExpanded(false)}
          >
            收起
          </button>
        </div>
        {views
          .slice()
          .reverse()
          .map((view) => (
            <article
              className="cluster-round-card"
              key={view.round}
              aria-label={`第 ${view.round} 轮`}
            >
              <header className="cluster-round-card-head">
                <span className="cluster-round-badge">第 {view.round} 轮</span>
                {view.round === currentRound ? (
                  <span className="cluster-round-current">
                    {roundFinished ? "本轮已结束" : "本轮进行中"}
                  </span>
                ) : null}
              </header>
              <p className="cluster-round-card-label">
                {view.round === 1 ? "初始需求" : "本轮要求"}
              </p>
              <p className="cluster-round-card-text">{requirementText(view)}</p>
              {view.summary ? (
                <>
                  <p className="cluster-round-card-label">本轮结论</p>
                  <p className="cluster-round-card-summary" title={view.summary}>
                    {view.summary}
                  </p>
                </>
              ) : view.round === currentRound && !roundFinished ? (
                <p className="cluster-round-card-note">
                  本轮进行中：结束后这里会显示结论。
                </p>
              ) : null}
            </article>
          ))}
      </div>
    );
  }

  return (
    <div className="cluster-round-stack">
      <div className="cluster-round-stack-cards">
        {preview.map((view) => (
          <div className="cluster-round-stack-card" key={view.round}>
            <span className="cluster-round-badge">第 {view.round} 轮</span>
            <span className="cluster-round-stack-text">
              {requirementText(view)}
            </span>
          </div>
        ))}
      </div>
      <button
        className="cluster-round-toggle"
        type="button"
        onClick={() => setExpanded(true)}
      >
        展开全部 {views.length} 轮
      </button>
    </div>
  );
}
