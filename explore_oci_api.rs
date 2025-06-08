// Temporary file to explore oci-client API
use oci_client::{Client, Reference};
use oci_client::client::ClientConfig;
use oci_client::secrets::RegistryAuth;

fn main() {
    // Let's see what methods are available on Client
    let config = ClientConfig::default();
    let client = Client::new(config);
    
    // Try to discover available methods
    // client.
}
