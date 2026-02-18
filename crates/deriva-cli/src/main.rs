mod args;
mod client;

use args::{Cli, Command};
use clap::Parser;
use client::proto::*;
use client::*;
use std::io::{self, Read, Write};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut client = DerivaClient::connect(cli.server).await?;

    match cli.command {
        Command::Put { source } => {
            let data = if source == "-" {
                let mut buf = Vec::new();
                io::stdin().read_to_end(&mut buf)?;
                buf
            } else {
                std::fs::read(&source)?
            };
            let resp = client.put_leaf(PutLeafRequest { data }).await?.into_inner();
            println!("{}", addr_to_hex(&resp.addr));
        }
        Command::Recipe { function, version, input, param } => {
            let inputs: Vec<Vec<u8>> = input
                .iter()
                .map(|h| hex_to_addr(h))
                .collect::<Result<_, _>>()?;
            let params: std::collections::HashMap<String, String> = param
                .iter()
                .map(|p| parse_param(p))
                .collect::<Result<_, _>>()?;
            let resp = client
                .put_recipe(PutRecipeRequest {
                    function_name: function,
                    function_version: version,
                    inputs,
                    params,
                })
                .await?
                .into_inner();
            println!("{}", addr_to_hex(&resp.addr));
        }
        Command::Get { addr, output } => {
            let addr_bytes = hex_to_addr(&addr)?;
            let mut stream = client.get(GetRequest { addr: addr_bytes }).await?.into_inner();
            let mut writer: Box<dyn Write> = match output {
                Some(path) => Box::new(std::fs::File::create(path)?),
                None => Box::new(io::stdout().lock()),
            };
            while let Some(chunk) = stream.next().await {
                writer.write_all(&chunk?.chunk)?;
            }
            writer.flush()?;
        }
        Command::Resolve { addr } => {
            let addr_bytes = hex_to_addr(&addr)?;
            let resp = client.resolve(ResolveRequest { addr: addr_bytes }).await?.into_inner();
            if !resp.found {
                eprintln!("not found");
                std::process::exit(1);
            }
            println!("function: {}/{}", resp.function_name, resp.function_version);
            for (i, input) in resp.inputs.iter().enumerate() {
                println!("input[{}]: {}", i, addr_to_hex(input));
            }
            for (k, v) in &resp.params {
                println!("param.{}: {}", k, v);
            }
        }
        Command::Invalidate { addr, cascade, dry_run, detail } => {
            let addr_bytes = hex_to_addr(&addr)?;
            if cascade || dry_run {
                let policy = if dry_run { "dry_run" } else { "immediate" }.to_string();
                let resp = client
                    .cascade_invalidate(CascadeInvalidateRequest {
                        addr: addr_bytes,
                        policy,
                        include_root: true,
                        detail_addrs: detail,
                    })
                    .await?
                    .into_inner();
                println!("evicted:   {}", resp.evicted_count);
                println!("traversed: {}", resp.traversed_count);
                println!("max_depth: {}", resp.max_depth);
                println!("reclaimed: {} bytes", resp.bytes_reclaimed);
                println!("duration:  {}μs", resp.duration_micros);
                if detail {
                    for a in &resp.evicted_addrs {
                        println!("  - {}", addr_to_hex(a));
                    }
                }
            } else {
                let resp = client
                    .invalidate(InvalidateRequest { addr: addr_bytes, cascade: false })
                    .await?
                    .into_inner();
                println!("was_cached: {}", resp.was_cached);
            }
        }
        Command::Status => {
            let resp = client.status(StatusRequest {}).await?.into_inner();
            println!("recipes:       {}", resp.recipe_count);
            println!("blobs:         {}", resp.blob_count);
            println!("cache entries: {}", resp.cache_entries);
            println!("cache size:    {} bytes", resp.cache_size_bytes);
            println!("hit rate:      {:.1}%", resp.cache_hit_rate * 100.0);
        }
    }

    Ok(())
}
