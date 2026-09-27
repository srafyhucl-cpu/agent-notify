-- 编排任务持久化（P1-1，B 方案）：只增表，不动现有任何表。
-- a2a_task_json 保存 A2A Task 完整 JSON（唯一事实源，§8.3「不建第二套」）；
-- task_id / workflow_id / goal / created_at 是查询与排序用的索引列，业务状态一律读 a2a_task_json。
CREATE TABLE orc_tasks (
    task_id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    goal TEXT NOT NULL,
    a2a_task_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_orc_tasks_created_at ON orc_tasks(created_at DESC);