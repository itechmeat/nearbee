//! The market seam. Its methods arrive in stage S2.

/// Everything Nearbee needs from an agent market. Implemented by
/// `nearbee-market`; the core depends only on this trait.
pub trait Market: Send + Sync {}
