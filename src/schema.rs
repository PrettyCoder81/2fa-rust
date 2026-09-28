// @generated automatically by Diesel CLI.

diesel::table! {
    recovery_codes (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 255]
        code_hash -> Varchar,
        used_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    security_events (id) {
        id -> Uuid,
        user_id -> Nullable<Uuid>,
        #[max_length = 100]
        event_type -> Varchar,
        #[max_length = 45]
        ip_address -> Nullable<Varchar>,
        #[max_length = 500]
        user_agent -> Nullable<Varchar>,
        metadata -> Nullable<Jsonb>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    sessions (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 255]
        token_hash -> Varchar,
        expires_at -> Timestamp,
        created_at -> Timestamp,
        last_used_at -> Timestamp,
        revoked_at -> Nullable<Timestamp>,
        #[max_length = 45]
        ip_address -> Nullable<Varchar>,
        #[max_length = 500]
        user_agent -> Nullable<Varchar>,
        #[max_length = 100]
        device_name -> Nullable<Varchar>,
    }
}

diesel::table! {
    two_factor_challenges (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 255]
        token_hash -> Varchar,
        expires_at -> Timestamp,
        created_at -> Timestamp,
    }
}

diesel::table! {
    two_factor_secrets (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 255]
        secret -> Varchar,
        created_at -> Timestamp,
        confirmed_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        #[max_length = 255]
        email -> Varchar,
        #[max_length = 255]
        username -> Varchar,
        password_hash -> Text,
        #[max_length = 255]
        firstname -> Nullable<Varchar>,
        #[max_length = 255]
        lastname -> Nullable<Varchar>,
        #[max_length = 255]
        avatar -> Varchar,
        two_factor_enabled -> Bool,
        created_at -> Timestamp,
    }
}

diesel::joinable!(recovery_codes -> users (user_id));
diesel::joinable!(security_events -> users (user_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(two_factor_challenges -> users (user_id));
diesel::joinable!(two_factor_secrets -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    recovery_codes,
    security_events,
    sessions,
    two_factor_challenges,
    two_factor_secrets,
    users,
);
