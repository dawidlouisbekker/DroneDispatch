//! Internal gRPC API (`proto/dronedrop/auth/v1/auth.proto`).

use contracts::dronedrop::auth::v1::{
    BatchGetUsersRequest, BatchGetUsersResponse, GetUserRequest, GetUserResponse,
    ListRevokedGrantsRequest, ListRevokedGrantsResponse, RevokedGrant, User,
    grant_registry_server::{GrantRegistry, GrantRegistryServer},
    user_directory_server::{UserDirectory, UserDirectoryServer},
};
use sqlx::PgPool;
use time::OffsetDateTime;
use tonic::{Request, Response, Status};
use uuid::Uuid;

pub fn routes(db: PgPool) -> tonic::transport::server::Router {
    tonic::transport::Server::builder()
        .add_service(UserDirectoryServer::new(Directory { db: db.clone() }))
        .add_service(GrantRegistryServer::new(Grants { db }))
}

type UserRow = (
    Uuid,
    String,
    Option<OffsetDateTime>,
    bool,
    OffsetDateTime,
    Option<OffsetDateTime>,
);

fn user(row: UserRow) -> User {
    let (sub, email, email_verified_at, mfa_enrolled, created_at, disabled_at) = row;
    User {
        sub: sub.to_string(),
        email,
        email_verified: email_verified_at.is_some(),
        mfa_enrolled,
        created_at: Some(timestamp(created_at)),
        disabled_at: disabled_at.map(timestamp),
    }
}

fn timestamp(t: OffsetDateTime) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: t.unix_timestamp(),
        nanos: t.nanosecond() as i32,
    }
}

fn internal(error: sqlx::Error) -> Status {
    tracing::error!(%error, "gRPC query failed");
    Status::internal("database error")
}

pub struct Directory {
    db: PgPool,
}

#[tonic::async_trait]
impl UserDirectory for Directory {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<GetUserResponse>, Status> {
        let sub: Uuid = request
            .into_inner()
            .sub
            .parse()
            .map_err(|_| Status::invalid_argument("sub must be a UUID"))?;
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT users.id, users.email, users.email_verified_at,
                    EXISTS (SELECT 1 FROM passkeys WHERE user_id = users.id),
                    users.created_at, users.disabled_at
             FROM users WHERE users.id = $1",
        )
        .bind(sub)
        .fetch_optional(&self.db)
        .await
        .map_err(internal)?;
        let user = row
            .map(user)
            .ok_or_else(|| Status::not_found("USER_NOT_FOUND"))?;
        Ok(Response::new(GetUserResponse { user: Some(user) }))
    }

    async fn batch_get_users(
        &self,
        request: Request<BatchGetUsersRequest>,
    ) -> Result<Response<BatchGetUsersResponse>, Status> {
        let subs: Vec<Uuid> = request
            .into_inner()
            .subs
            .iter()
            .filter_map(|sub| sub.parse().ok())
            .collect();
        let rows: Vec<UserRow> = sqlx::query_as(
            "SELECT users.id, users.email, users.email_verified_at,
                    EXISTS (SELECT 1 FROM passkeys WHERE user_id = users.id),
                    users.created_at, users.disabled_at
             FROM users WHERE users.id = ANY ($1)",
        )
        .bind(&subs)
        .fetch_all(&self.db)
        .await
        .map_err(internal)?;
        Ok(Response::new(BatchGetUsersResponse {
            users: rows.into_iter().map(user).collect(),
        }))
    }
}

pub struct Grants {
    db: PgPool,
}

/// Access tokens live an hour, so recent revocations are few; one page is enough.
const MAX_REVOKED_GRANTS: i64 = 1000;

#[tonic::async_trait]
impl GrantRegistry for Grants {
    async fn list_revoked_grants(
        &self,
        request: Request<ListRevokedGrantsRequest>,
    ) -> Result<Response<ListRevokedGrantsResponse>, Status> {
        let since = request
            .into_inner()
            .revoked_since
            .map(|t| OffsetDateTime::from_unix_timestamp(t.seconds))
            .transpose()
            .map_err(|_| Status::invalid_argument("revoked_since is out of range"))?
            .unwrap_or_else(|| OffsetDateTime::now_utc() - time::Duration::hours(1));
        let rows: Vec<(Uuid, Uuid, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, user_id, revoked_at FROM grants WHERE revoked_at >= $1 ORDER BY revoked_at LIMIT $2",
        )
        .bind(since)
        .bind(MAX_REVOKED_GRANTS)
        .fetch_all(&self.db)
        .await
        .map_err(internal)?;
        let grants = rows
            .into_iter()
            .map(|(grant_id, sub, revoked_at)| RevokedGrant {
                grant_id: grant_id.to_string(),
                sub: sub.to_string(),
                revoked_at: Some(timestamp(revoked_at)),
            })
            .collect();
        Ok(Response::new(ListRevokedGrantsResponse {
            grants,
            page: None,
        }))
    }
}
