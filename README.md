# Docker Image Pusher

Docker Image Pusher is a command-line tool written in Rust that allows users to push Docker image tar packages directly to a Docker registry. This tool is designed to handle large images efficiently, including those larger than 10GB, by utilizing chunked uploads.
## [中文文档](README_zh.md)
## Features

- **Chunked Uploads**: Supports uploading large Docker images in chunks, ensuring stability and reliability during the upload process.
- **Docker Registry API Interaction**: Directly interacts with the Docker registry API for seamless image uploads.
- **Authentication Support**: Handles authentication with the Docker registry, including token retrieval and session management.
- **Progress Tracking**: Provides real-time feedback on upload progress to the user.

## Installation

To install Docker Image Pusher, you can either download the pre-built binary or build it from source.

### Pre-built Binary

Download the latest release from the [Releases page](https://github.com/yorelog/docker-image-pusher/releases).

### Build from Source

To build the project from source, ensure you have Rust and Cargo installed, then run the following commands:

```bash
git clone https://github.com/yorelog/docker-image-pusher.git
cd docker-image-pusher
cargo build --release
```

## Usage

To push a Docker image tar package to a registry, use the following command:

```bash
./target/release/docker-image-pusher \
  --address http://your-registry-address:5000 \
  --username your-username \
  --password your-password \
  --project your-project \
  --file /path/to/your-image.tar
```

### Command-Line Arguments

| Argument     | Description                                               |
|--------------|-----------------------------------------------------------|
| `--address`  | The address of the target Docker registry.                |
| `--username` | The username for authentication (optional).               |
| `--password` | The password for authentication (optional).               |
| `--project`  | The target project name in the registry (optional).      |
| `--file`     | The path to the local Docker image tar package (required).|
| `--skipTls`  | Whether to skip TLS verification (optional).              |
| `--chunkSize`| The size of each chunk for uploads (in bytes, required for chunked uploads). |

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on the [GitHub repository](https://github.com/yorelog/docker-image-pusher).

## License

This project is licensed under the MIT License. See the LICENSE file for more details.