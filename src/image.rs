/// Sanitizes image names for use as directory names
///
/// Docker image names can contain characters that are not valid in file paths.
/// This function replaces problematic characters with underscores to create
/// safe directory names for the cache.
///
/// # Replacements
///
/// - `/` → `_` (registry separators)  
/// - `:` → `_` (tag separators)
/// - `@` → `_` (digest separators)
///
/// # Examples
///
/// ```
/// assert_eq!(sanitize_image_name("nginx:latest"), "nginx_latest");
/// assert_eq!(sanitize_image_name("registry.example.com/app:v1.0"), "registry.example.com_app_v1.0");
/// ```
///
/// # Arguments
///
/// * `image_name` - Original image name with potentially unsafe characters
///
/// # Returns
///
/// `String` - Sanitized name safe for use as directory name
pub fn sanitize_image_name(image_name: &str) -> String {
    image_name
        .replace("/", "_") // Replace registry/namespace separators
        .replace(":", "_") // Replace tag separators
        .replace("@", "_") // Replace digest separators
}
