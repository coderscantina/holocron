//! Measures replay retention and heatmap folding at a configurable scale, through the same store
//! and fold functions the server's maintenance runs. Development only.
//!
//! `cargo run --release -p holocron-server --example recording_maintenance -- <scratch-dir> [captures] [interactions]`
//!
//! Writes `captures` replays of five 64 KiB parts each (rrweb-shaped JSON with realistic
//! repetition), removes them all as retention would, and reports the slowest removal step,
//! vacuum time and file sizes. Then admits `interactions` heatmap interactions through the inbox,
//! folds them, and times one 90-day cell read.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use holocron_protocol::operations::recordings::{HeatmapCells, ViewportClass};
use holocron_server::recordings::heatmaps;
use holocron_server::recordings::replay::Summary;
use holocron_server::recordings::store::{Client, Part, Store};
use holocron_storage::{Budgets, HeatmapInteraction, Interaction, Lane, Record, Storage};

const PART_BYTES: usize = 65_536;
const PARTS: i64 = 5;
const DAY_MS: i64 = 86_400_000;

/// One part: an rrweb mutation event whose text varies per capture and part, padded with markup
/// that repeats as real DOM does.
fn part_json(capture: u64, part: i64) -> String {
    let mut text = String::with_capacity(PART_BYTES);
    let mut node = 0u64;
    while text.len() < PART_BYTES - 200 {
        node += 1;
        text.push_str(&format!(
            r#"{{"parentId":{p},"nextId":null,"node":{{"type":2,"tagName":"div","attributes":{{"class":"card c{c}"}},"childNodes":[{{"type":3,"textContent":"****** *** {n}","id":{id}}}],"id":{id2}}}}},"#,
            p = node % 97,
            c = (capture + node) % 13,
            n = capture * 31 + node * 7 + part as u64,
            id = node * 2,
            id2 = node * 2 + 1
        ));
    }
    text.pop();
    format!(
        r#"[{{"type":3,"timestamp":{},"data":{{"source":0,"texts":[],"attributes":[],"removes":[],"adds":[{text}]}}}}]"#,
        1_790_000_000_000i64 + part
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("usage: <scratch-dir> [captures] [interactions]"))?,
    );
    let captures: u64 = args.next().map_or(Ok(2_000), |a| a.parse())?;
    let interactions: u64 = args.next().map_or(Ok(100_000), |a| a.parse())?;
    std::fs::create_dir_all(&dir)?;

    let store = Store::open(&dir).await?;
    let client = Client::default();
    let site = 1u128;
    let started = Instant::now();
    let mut raw = 0u64;
    for capture in 0..captures {
        for part in 0..PARTS {
            let json = part_json(capture, part);
            raw += json.len() as u64;
            let summary = Summary {
                events: 1,
                first_ms: Some(1_790_000_000_000),
                last_ms: Some(1_790_000_000_000 + part),
                pages: vec!["/".into()],
            };
            let payload = zstd::bulk::compress(json.as_bytes(), 3)?;
            let fingerprint: [u8; 16] =
                blake3::hash(json.as_bytes()).as_bytes()[..16].try_into()?;
            store
                .put(
                    Part {
                        site,
                        capture: u128::from(capture) + 1,
                        visitor: None,
                        chunk: part,
                        received_at: 1_790_000_000_000,
                        summary: &summary,
                        raw_bytes: json.len() as i64,
                        payload,
                        fingerprint,
                        client: &client,
                    },
                    &[],
                    u64::MAX,
                )
                .await?;
        }
    }
    let usage = store.usage().await?;
    println!(
        "replay: {captures} captures, {} parts, raw {:.1} MiB, stored {:.1} MiB, file {:.1} MiB, write {:.1} s ({:.0} parts/s)",
        captures as i64 * PARTS,
        raw as f64 / 1048576.0,
        usage.bytes as f64 / 1048576.0,
        usage.file_bytes as f64 / 1048576.0,
        started.elapsed().as_secs_f64(),
        (captures as i64 * PARTS) as f64 / started.elapsed().as_secs_f64()
    );

    let (mut steps, mut slowest, started) = (0, Duration::ZERO, Instant::now());
    loop {
        let step = Instant::now();
        let removed = store.expire(site, i64::MAX, false, 32).await?;
        slowest = slowest.max(step.elapsed());
        steps += 1;
        if removed < 32 {
            break;
        }
    }
    let removal = started.elapsed();
    let started = Instant::now();
    let mut vacuum_steps = 0;
    while store.vacuum(1024).await? {
        vacuum_steps += 1;
    }
    println!(
        "retention: {steps} steps of 32 captures, slowest {:.1} ms, total {:.2} s; vacuum {vacuum_steps} steps, {:.2} s; file after {:.1} MiB",
        slowest.as_secs_f64() * 1000.0,
        removal.as_secs_f64(),
        started.elapsed().as_secs_f64(),
        store.usage().await?.file_bytes as f64 / 1048576.0
    );
    store.close().await;

    let storage = Storage::open(
        &dir,
        Budgets {
            disk_reserve_bytes: 0,
            ..Budgets::default()
        },
    )
    .await?;
    heatmaps::create(&storage).await?;
    let now = 1_790_000_000_000i64;
    let started = Instant::now();
    let batch: Vec<Record> = (0..interactions)
        .map(|i| {
            Record::HeatmapInteraction(HeatmapInteraction {
                id: u128::from(i) + 1,
                site,
                capture_id: u128::from(i / 10) + 1,
                event_time_ms: now - (i % 90) as i64 * DAY_MS,
                receive_time_ms: now - (i % 90) as i64 * DAY_MS,
                path: format!("/page-{}", i % 50),
                viewport_width: [1440, 820, 390][(i % 3) as usize],
                viewport_height: 900,
                kind: if i % 5 == 4 {
                    Interaction::Scroll {
                        percent: (i % 101) as u8,
                    }
                } else {
                    Interaction::Click {
                        x: (i * 37 % 1440) as u16,
                        page_y: (i * 53 % 3000) as u32,
                    }
                },
            })
        })
        .collect();
    for chunk in batch.chunks(10_000) {
        storage
            .admit(chunk)
            .await
            .map_err(|refusal| anyhow::anyhow!("{refusal}"))?;
    }
    storage.drain().await;
    let admitted = started.elapsed();

    let live = Instant::now();
    let before = read_cells(&storage, site, now).await?;
    let live = live.elapsed();
    let (mut steps, mut slowest, started) = (0, Duration::ZERO, Instant::now());
    loop {
        let step = Instant::now();
        let folded = heatmaps::fold(&storage, now + DAY_MS, 10_000).await?;
        slowest = slowest.max(step.elapsed());
        steps += 1;
        if folded < 10_000 {
            break;
        }
    }
    let fold = started.elapsed();
    let cells_read = Instant::now();
    let after = read_cells(&storage, site, now).await?;
    let cells_read = cells_read.elapsed();
    let cells: i64 = storage
        .read(Lane::Report, |s| {
            s.connection()
                .query_row("SELECT count(*) FROM heatmap_cells", [], |r| r.get(0))
        })
        .await?
        .value;
    assert_eq!(before.total_clicks, after.total_clicks);
    println!(
        "heatmaps: {interactions} interactions admitted and applied in {:.2} s; read before folding {:.1} ms; fold {steps} steps of 10,000 captures, slowest {:.1} ms, total {:.2} s; {cells} cells; read after folding {:.1} ms; clicks on the page {}",
        admitted.as_secs_f64(),
        live.as_secs_f64() * 1000.0,
        slowest.as_secs_f64() * 1000.0,
        fold.as_secs_f64(),
        cells_read.as_secs_f64() * 1000.0,
        after.total_clicks
    );
    storage.close().await?;
    Ok(())
}

/// Click cells of one page over 90 days, as `heatmaps.cells` reads them.
async fn read_cells(storage: &Storage, site: u128, now: i64) -> anyhow::Result<HeatmapCells> {
    let last = heatmaps::utc_day(now);
    Ok(storage
        .read(Lane::Report, move |s| {
            heatmaps::cells(
                s.connection(),
                site,
                "/page-7",
                ViewportClass::Desktop,
                last - 90,
                last,
            )
        })
        .await?
        .value)
}
