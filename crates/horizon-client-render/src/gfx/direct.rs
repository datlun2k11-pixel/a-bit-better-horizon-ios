//! Showing the newest frame from a thread of its own.
//!
//! iOS composites only at the display's refresh, and a CAMetalLayer hands its
//! drawables back only then: a loop that acquires one per frame runs at the
//! refresh rate whatever its present mode, so with VSync off the frame counter
//! could not show how fast the GPU really is. Here the loop draws into frames of
//! its own and this thread copies the newest one onto the swapchain whenever the
//! display gives it a drawable: the loop runs at the GPU's pace, frames it
//! outruns are dropped, and the screen shows the newest one each refresh.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;

/// Frames the loop can be ahead of the display by: six cover a 360 fps loop on
/// a 60 Hz display, like frame generation's.
const FRAMES: usize = 6;

/// A frame to draw into, like the swapchain's.
fn frame(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Direct frame"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &config.view_formats,
    })
}

/// The thread that copies the newest frame to the swapchain and presents it.
pub(super) struct Present {
    config: wgpu::SurfaceConfiguration,
    send: Option<mpsc::Sender<wgpu::Texture>>,
    free: mpsc::Receiver<wgpu::Texture>,
    /// The surface gave something other than a frame: the renderer takes it back.
    failed: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Present {
    pub(super) fn start(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: Arc<wgpu::Surface<'static>>,
        config: &wgpu::SurfaceConfiguration,
    ) -> Self {
        let (send, receive) = mpsc::channel::<wgpu::Texture>();
        let (give, free) = mpsc::channel();
        for _ in 0..FRAMES {
            let _ = give.send(frame(device, config));
        }
        let (device, queue) = (device.clone(), queue.clone());
        let failed = Arc::new(AtomicBool::new(false));
        let failed_here = failed.clone();
        let thread = thread::Builder::new()
            .name("Direct present".into())
            .spawn(move || {
                while let Ok(mut newest) = receive.recv() {
                    // Every frame but the newest would never be shown: give those back
                    // for the loop to draw over instead of having them wait a refresh.
                    while let Ok(newer) = receive.try_recv() {
                        let _ = give.send(newest);
                        newest = newer;
                    }
                    if !failed_here.load(Ordering::Relaxed) {
                        // The display gives a drawable once per refresh: that wait is
                        // here, not in the loop. Anything but a frame (outdated, lost,
                        // occluded...) stops this thread; a suboptimal frame still shows.
                        let target = match surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(t) => Some(t),
                            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                                failed_here.store(true, Ordering::Relaxed);
                                Some(t)
                            }
                            _ => {
                                failed_here.store(true, Ordering::Relaxed);
                                None
                            }
                        };
                        if let Some(target) = target {
                            let mut encoder = device.create_command_encoder(&Default::default());
                            encoder.copy_texture_to_texture(
                                newest.as_image_copy(),
                                target.texture.as_image_copy(),
                                target.texture.size(),
                            );
                            queue.submit([encoder.finish()]);
                            queue.present(target);
                        }
                    }
                    let _ = give.send(newest);
                }
            })
            .expect("spawn the direct present thread");
        Self {
            config: config.clone(),
            send: Some(send),
            free,
            failed,
            thread: Some(thread),
        }
    }

    /// What another size, format or present mode needs: stopped and started again.
    pub(super) fn matches(&self, config: &wgpu::SurfaceConfiguration) -> bool {
        self.config == *config
    }

    /// The surface failed it or the thread died (a panic).
    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed) || self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    /// A frame to draw into: waits only once the loop is `FRAMES` ahead of the display.
    pub(super) fn frame(&self) -> Option<wgpu::Texture> {
        self.free.recv().ok()
    }

    /// Hands a drawn frame over: it shows at the next refresh the display gives.
    pub(super) fn present(&self, texture: wgpu::Texture) {
        if let Some(send) = &self.send {
            let _ = send.send(texture);
        }
    }
}

impl Drop for Present {
    fn drop(&mut self) {
        // The thread then leaves its wait for a frame and joins after the one it shows.
        self.send = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
