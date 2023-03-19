use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use video_rs::{Encoder, EncoderSettings, Locator, Time, Url};

use clap::Parser;

#[derive(Parser, Debug)]
#[command()]
struct Encode {
	#[arg(long)]
	url: Url,

	#[arg(long, default_value_t = 800)]
	width: u32,

	#[arg(long, default_value_t = 600)]
	height: u32,

	#[arg(long, default_value_t = false)]
	headless: bool,

	#[arg(long)]
	output: Option<PathBuf>,
}

#[derive(Parser, Debug)]
enum Args {
	Encode(Encode),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	tracing_subscriber::fmt::init();

	let args = Args::parse();

	let Args::Encode(args) = args;

	// create a `Browser` that spawns a `chromium` process running with UI (`with_head()`, headless is default)
	// and the handler that drives the websocket etc.
	let (mut browser, mut handler) = Browser::launch({
		let mut builder = BrowserConfig::builder().window_size(args.width, args.height);

		if !args.headless {
			builder = builder.with_head()
		}
		builder.build()?
	})
	.await?;

	// spawn a new task that continuously polls the handler
	let handle = tokio::task::spawn(async move {
		while let Some(h) = handler.next().await {
			if h.is_err() {
				break;
			}
		}
	});

	let page = browser.new_page(args.url.clone()).await?;

	page.wait_for_navigation().await?;

	let _ = page
		.execute(
			chromiumoxide::cdp::browser_protocol::page::StartScreencastParams::builder()
				.every_nth_frame(1)
				.format(chromiumoxide::cdp::browser_protocol::page::StartScreencastFormat::Jpeg)
				.build(),
		)
		.await;

	let mut listener = page
		.event_listener::<chromiumoxide::cdp::browser_protocol::page::EventScreencastFrame>()
		.await?;

	let destination: Locator = args
		.output
		.unwrap_or_else(|| {
			let mut hostname = PathBuf::from(args.url.host_str().unwrap());
			hostname.set_extension("mp4");
			hostname
		})
		.into();
	video_rs::init().unwrap();

	let settings = EncoderSettings::for_h264_yuv420p(1600, 1200, true);
	let mut encoder = Encoder::new(&destination, settings).expect("failed to create encoder");

	let mut prev_duration: Option<Duration> = None;
	let mut position = Time::zero();

	while let Some(item) = listener.next().await {
		let time = std::time::Instant::now();
		let buffer = base64::engine::general_purpose::STANDARD
			.decode(AsRef::<[u8]>::as_ref(&item.data))
			.unwrap();

		tracing::info!("{}: {}ms", "base64", time.elapsed().as_millis());

		let time = std::time::Instant::now();
		let image = image::load_from_memory_with_format(&buffer, image::ImageFormat::Jpeg).unwrap();
		let image = image.to_rgb8();

		tracing::info!("{}: {}ms", "image::load", time.elapsed().as_millis());

		let time = std::time::Instant::now();
		let frame = nshare::ToNdarray3::into_ndarray3(image);
		let frame = frame.permuted_axes([1, 2, 0]);

		tracing::info!("{}: {}ms", "ndarray", time.elapsed().as_millis());

		println!("frame {:?}", frame.dim());

		let ts = std::time::Duration::from_nanos(
			(*item.metadata.timestamp.as_ref().unwrap().inner() * 1000000000.0) as u64,
		);

		if let Some(prev) = prev_duration.as_mut() {
			let delta = ts - *prev;
			position = position.aligned_with(&delta.into()).add();
		}

		prev_duration = Some(ts);

		let time = std::time::Instant::now();
		encoder
			.encode(&frame, &position)
			.expect("failed to encode frame");

		tracing::info!("{}: {}ms", "encoder::encode", time.elapsed().as_millis());

		page.execute(
			chromiumoxide::cdp::browser_protocol::page::ScreencastFrameAckParams::builder()
				.session_id(item.session_id)
				.build()
				.unwrap(),
		)
		.await?;
	}

	encoder.finish().expect("Failed ");

	browser.close().await?;
	let _ = handle.await;
	Ok(())
}
