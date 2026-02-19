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
        Command::Gc { dry_run, grace_period, detail, max_removals } => {
            let resp = client.garbage_collect(GcRequest {
                grace_period_secs: grace_period,
                dry_run,
                detail_addrs: detail,
                max_removals,
            }).await?.into_inner();
            let mode = if resp.dry_run { "DRY RUN" } else { "COMPLETED" };
            println!("Garbage Collection {mode}");
            println!("  Blobs removed:   {}", resp.blobs_removed);
            println!("  Recipes removed: {}", resp.recipes_removed);
            println!("  Cache cleared:   {}", resp.cache_entries_removed);
            println!("  Bytes reclaimed: {} ({} blobs + {} cache)",
                resp.total_bytes_reclaimed, resp.bytes_reclaimed_blobs, resp.bytes_reclaimed_cache);
            println!("  Live blobs:      {}", resp.live_blobs);
            println!("  Live recipes:    {}", resp.live_recipes);
            println!("  Pinned addrs:    {}", resp.pinned_count);
            println!("  Duration:        {}μs", resp.duration_micros);
            if detail {
                for a in &resp.removed_addrs {
                    println!("  - {}", addr_to_hex(a));
                }
            }
        }
        Command::Pin { addr } => {
            let addr_bytes = hex_to_addr(&addr)?;
            let resp = client.pin(PinRequest { addr: addr_bytes }).await?.into_inner();
            println!("pinned: {} (new: {})", addr, resp.was_new);
        }
        Command::Unpin { addr } => {
            let addr_bytes = hex_to_addr(&addr)?;
            let resp = client.unpin(UnpinRequest { addr: addr_bytes }).await?.into_inner();
            println!("unpinned: {} (was_pinned: {})", addr, resp.was_pinned);
        }
        Command::ListPins => {
            let resp = client.list_pins(ListPinsRequest {}).await?.into_inner();
            println!("Pinned addrs ({}):", resp.count);
            for a in &resp.addrs {
                println!("  {}", addr_to_hex(a));
            }
        }
    }

    Ok(())
}
