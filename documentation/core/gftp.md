# Golem File Transfer Protocol (gftp)

The Golem File Transfer Protocol (gftp) is a custom file transfer protocol designed for efficient and secure file transfers between nodes in the Yagna network. It is optimized for the decentralized nature of the Golem ecosystem and provides features tailored to the needs of distributed computing tasks.

## Key Features

1. **Efficient Transfers**: Optimized for transferring large files and datasets across the network.
2. **Chunked Transfers**: Supports splitting files into chunks for parallel transfer and improved reliability.
3. **Resume Capability**: Allows interrupted transfers to be resumed from the last successfully transferred chunk.
4. **Integrity Checking**: Implements checksums to verify the integrity of transferred files.
5. **Encryption**: Provides end-to-end encryption for secure file transfers.
6. **Deduplication**: Avoids transferring duplicate data when possible.

## Transfer Workflow

1. **Initialization**: The sender initiates a transfer by providing file metadata to the receiver.
2. **Chunking**: Large files are split into manageable chunks.
3. **Transfer**: Chunks are transferred in parallel or sequentially, depending on network conditions.
4. **Verification**: Each chunk is verified for integrity upon receipt.
5. **Reassembly**: The receiver reassembles the chunks into the complete file.
6. **Final Verification**: The complete file is verified against the original checksum.

## Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Sender Node" as SN
RECTANGLE "Receiver Node" as RN
RECTANGLE "GFTP" as GFTP {
  RECTANGLE "Chunker" as CH
  RECTANGLE "Transfer Manager" as TM
  RECTANGLE "Integrity Checker" as IC
  RECTANGLE "Encryption Module" as EM
}
RECTANGLE "Network Layer" as NL

SN --> GFTP : Initiates transfer
GFTP --> RN : Delivers file
CH --> GFTP : Splits files
TM --> GFTP : Manages transfer
IC --> GFTP : Verifies integrity
EM --> GFTP : Encrypts/Decrypts
GFTP --> NL : Uses for transmission

@enduml
\```

## Integration with Other Components

The gftp component interacts with several other Yagna components:

1. **Network (net)**: Utilizes the network layer for actual data transfer.
2. **Cryptography (crypto)**: Uses cryptographic functions for encryption and integrity checking.
3. **Activity Management**: Facilitates file transfers related to compute tasks.
4. **ExeUnit**: Enables file transfers to and from isolated execution environments.

## Code Example: Transferring a File

Here's a simplified example of how the gftp component might be used to transfer a file:

\```rust
use ya_gftp::{GftpTransfer, TransferProgress};

async fn transfer_file(
    gftp: &dyn GftpTransfer,
    source_path: &str,
    destination_address: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let transfer = gftp.send_file(source_path, destination_address).await?;

    while let Some(progress) = transfer.progress().await? {
        match progress {
            TransferProgress::Chunking { total_chunks } => {
                println!("Splitting file into {} chunks", total_chunks);
            }
            TransferProgress::Transferring { chunk, total_chunks } => {
                println!("Transferring chunk {} of {}", chunk, total_chunks);
            }
            TransferProgress::Verifying => {
                println!("Verifying transferred file");
            }
            TransferProgress::Completed => {
                println!("File transfer completed successfully");
                break;
            }
        }
    }

    Ok(())
}

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gftp = // Initialize GftpTransfer implementation
    let source_path = "/path/to/local/file.dat";
    let destination_address = "gftp://recipient_node_id/path/to/remote/file.dat";

    transfer_file(&gftp, source_path, destination_address).await?;
    Ok(())
}
\```

This example demonstrates:
1. Initiating a file transfer using the gftp component.
2. Monitoring the progress of the transfer, including chunking, transfer of individual chunks, and verification.
3. Handling the completion of the transfer.

The Golem File Transfer Protocol (gftp) component provides a reliable and efficient means of transferring files within the Yagna ecosystem, supporting the data transfer needs of distributed compute tasks and ensuring the integrity and security of transferred files.