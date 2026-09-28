CREATE TABLE security_events (
    id UUID PRIMARY KEY,
    user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    event_type VARCHAR(100) NOT NULL,
    ip_address VARCHAR(45) NULL,
    user_agent VARCHAR(500) NULL,
    metadata JSONB NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX security_events_user_id_idx
ON security_events(user_id);

CREATE INDEX security_events_event_type_idx
ON security_events(event_type);

CREATE INDEX security_events_created_at_idx
ON security_events(created_at);
