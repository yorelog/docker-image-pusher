/// Application-level reporting utilities for blob upload operations.
/// 
/// Low-level blob management functions are in oci_core::workflows::push_workflow.
/// This module provides application-specific reporting and UI feedback.


/// Report upload completion with summary statistics.
pub fn report_upload_summary(uploaded_count: usize, skipped_count: usize) {
    if skipped_count > 0 {
        println!(
            "💡 Skipped {} layer(s) that already existed in the registry",
            skipped_count
        );
    }
    if uploaded_count > 0 {
        println!("✅ Successfully uploaded {} layer(s)", uploaded_count);
    }
}
