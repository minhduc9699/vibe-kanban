-- Scheduled Tasks
CREATE TABLE scheduled_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    execute_at TEXT NOT NULL, -- ISO8601
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK(status IN ('pending','running','completed','failed','cancelled')),
    locked_until TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_scheduled_tasks_execute_at ON scheduled_tasks(status, execute_at);
CREATE INDEX idx_scheduled_tasks_task_id ON scheduled_tasks(task_id);

-- Notifications
CREATE TABLE notifications (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    notification_type TEXT NOT NULL
        CHECK(notification_type IN ('task_complete','approval_needed','question','error')),
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    payload TEXT, -- JSON
    read_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_notifications_session ON notifications(session_id, read_at);
CREATE INDEX idx_notifications_unread ON notifications(read_at) WHERE read_at IS NULL;
