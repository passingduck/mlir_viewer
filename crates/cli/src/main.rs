use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use trace_format::{fixture, PassNode, TraceReader};

#[derive(Parser)]
#[command(
    name = "mlir-viewer",
    version,
    about = "Visual debugger for MLIR pass pipelines"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve a trace file to the local web viewer
    Serve {
        file: PathBuf,
        #[arg(long, default_value = "127.0.0.1:3000")]
        listen: SocketAddr,
    },
    /// Inspect trace files
    Trace {
        #[command(subcommand)]
        command: TraceCmd,
    },
    /// Developer utilities
    Dev {
        #[command(subcommand)]
        command: DevCmd,
    },
}

#[derive(Subcommand)]
enum TraceCmd {
    /// Print trace metadata and the pass execution tree
    Dump { file: PathBuf },
}

#[derive(Subcommand)]
enum DevCmd {
    /// Write a deterministic demo trace (for development and tests)
    GenFixture {
        file: PathBuf,
        /// Emit a full-fidelity trace with a scripted op-identity stream.
        #[arg(long)]
        full: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Serve { file, listen } => serve(&file, listen).await,
        Cmd::Trace {
            command: TraceCmd::Dump { file },
        } => dump(&file),
        Cmd::Dev {
            command: DevCmd::GenFixture { file, full },
        } => {
            if full {
                fixture::write_full_demo_trace(&file)?;
            } else {
                fixture::write_demo_trace(&file)?;
            }
            println!("wrote {}", file.display());
            Ok(())
        }
    }
}

async fn serve(file: &std::path::Path, listen: SocketAddr) -> Result<()> {
    let app = server::router(file)?;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let address = listener.local_addr()?;
    eprintln!("mlir-viewer listening on http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn dump(file: &std::path::Path) -> Result<()> {
    let reader = TraceReader::open(file)?;
    println!("# meta");
    for (k, v) in reader.meta()? {
        println!("  {k} = {v}");
    }
    println!("# passes");
    for root in reader.passes()? {
        print_pass(&root, 0);
    }
    Ok(())
}

fn print_pass(node: &PassNode, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    let ms = (node.end_ns - node.start_ns) as f64 / 1_000_000.0;
    let marker = if node.ir_changed { "" } else { "  (no change)" };
    println!("{indent}{} — {ms:.2}ms{marker}", node.name);
    for child in &node.children {
        print_pass(child, depth + 1);
    }
}
