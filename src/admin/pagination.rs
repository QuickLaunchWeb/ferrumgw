use std::sync::Arc;
use hyper::{Request, Body};
use serde::{Serialize, Deserialize};

/// Pagination settings from query params
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationQuery {
    /// Page number (1-based)
    #[serde(default = "default_page")]
    pub page: usize,
    
    /// Number of items per page
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_page() -> usize {
    1
}

fn default_limit() -> usize {
    crate::config::env_config::EnvConfig::default_pagination_limit()
}

/// Response wrapper for paginated results
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    /// The paginated data
    pub data: Vec<T>,
    
    /// Pagination metadata
    pub pagination: PaginationMeta,
}

/// Pagination metadata for responses
#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    /// Current page number (1-based)
    pub page: usize,
    
    /// Number of items per page
    pub limit: usize,
    
    /// Total number of items across all pages
    pub total: usize,
    
    /// Total number of pages
    pub pages: usize,
}

impl PaginationQuery {
    /// Extract pagination parameters from request query string
    pub fn from_request(req: &Request<Body>) -> Self {
        let query_string = req.uri().query().unwrap_or("");
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query_string.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        
        let mut page = default_page();
        let mut limit = default_limit();
        
        for (k, v) in pairs {
            match k.as_str() {
                "page" => {
                    if let Ok(p) = v.parse::<usize>() {
                        if p > 0 {
                            page = p;
                        }
                    }
                },
                "limit" => {
                    if let Ok(l) = v.parse::<usize>() {
                        if l > 0 {
                            // Enforce max limit to prevent excessive requests
                            limit = std::cmp::min(l, 1000);
                        }
                    }
                },
                _ => {}
            }
        }
        
        Self { page, limit }
    }
    
    /// Calculate offset for database queries
    pub fn offset(&self) -> usize {
        (self.page - 1) * self.limit
    }
    
    /// Create pagination metadata
    pub fn create_meta(&self, total: usize) -> PaginationMeta {
        let pages = if total == 0 {
            1
        } else {
            (total + self.limit - 1) / self.limit
        };
        
        PaginationMeta {
            page: self.page,
            limit: self.limit,
            total,
            pages,
        }
    }
    
    /// Apply pagination to a vector of items
    pub fn paginate<T: Clone>(&self, items: &[T]) -> (Vec<T>, PaginationMeta) {
        let total = items.len();
        let offset = self.offset();
        
        let paginated_items = items.iter()
            .skip(offset)
            .take(self.limit)
            .cloned()
            .collect();
        
        let meta = self.create_meta(total);
        
        (paginated_items, meta)
    }
}

/// Create a paginated response for serialization
pub fn create_paginated_response<T: Serialize>(items: Vec<T>, meta: PaginationMeta) -> PaginatedResponse<T> {
    PaginatedResponse {
        data: items,
        pagination: meta,
    }
}
