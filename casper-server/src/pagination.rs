use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}

fn default_page() -> i64 { 1 }
fn default_per_page() -> i64 { 50 }

impl PaginationParams {
    pub fn offset(&self) -> i64 {
        ((self.page.max(1)) - 1) * self.per_page.clamp(1, 100)
    }

    pub fn limit(&self) -> i64 {
        self.per_page.clamp(1, 100)
    }
}

#[derive(Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

#[derive(Serialize)]
pub struct Pagination {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
}
