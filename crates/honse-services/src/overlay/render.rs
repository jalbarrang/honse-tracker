//! The D3D11 side of the overlay: swapchain in, egui painted on top.
//!
//! # The one rule that matters
//!
//! **Nothing derived from the backbuffer survives a frame.** A cached
//! `ID3D11RenderTargetView` holds a reference to the backbuffer, the game's
//! `ResizeBuffers` then fails, and the symptom is a resolution or fullscreen
//! switch that silently stops working. Every frame: get the buffer, make the
//! view, paint, drop. The device, context and `egui_directx11::Renderer` are
//! not backbuffer-derived and are cached.
//!
//! # Colour space
//!
//! egui blends in gamma space, so the render target must be viewed as
//! non-sRGB. The game's swapchain may well be `*_SRGB`, so the view is created
//! with the format explicitly downgraded ([`gamma_view_format`]) rather than
//! inherited — otherwise every panel comes out washed out.

use egui_directx11::{Renderer, RendererOutput};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D, D3D11_RENDER_TARGET_VIEW_DESC,
    D3D11_RENDER_TARGET_VIEW_DESC_0, D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_TEX2D_RTV, D3D11_TEXTURE2D_DESC,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_B8G8R8X8_UNORM,
    DXGI_FORMAT_B8G8R8X8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;

use super::d3d11_state::PipelineState;

/// Device-derived resources. Safe to keep across frames — none of these hold a
/// backbuffer reference.
pub struct Painter {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    renderer: Renderer,
}

impl Painter {
    /// Build the painter from the swapchain the present callback handed us.
    ///
    /// # Safety
    /// `swapchain` must be the live `IDXGISwapChain` from edge's present
    /// callback, on the render thread.
    pub unsafe fn new(swapchain: &IDXGISwapChain) -> windows::core::Result<Self> {
        // SAFETY: the swapchain is live and owned by the game's D3D11 device.
        let device: ID3D11Device = unsafe { swapchain.GetDevice() }?;
        // SAFETY: every D3D11 device has an immediate context.
        let context = unsafe { device.GetImmediateContext() }?;
        let renderer = Renderer::new(&device)?;
        Ok(Self {
            device,
            context,
            renderer,
        })
    }

    /// Backbuffer size in physical pixels, re-read every frame so a resize is
    /// picked up without any cached state to invalidate.
    ///
    /// # Safety
    /// As [`Painter::new`].
    pub unsafe fn backbuffer_size(swapchain: &IDXGISwapChain) -> windows::core::Result<(u32, u32)> {
        // SAFETY: buffer 0 always exists on a live swapchain.
        let backbuffer: ID3D11Texture2D = unsafe { swapchain.GetBuffer(0) }?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `desc` is a valid out-param for the texture's description.
        unsafe { backbuffer.GetDesc(&raw mut desc) };
        Ok((desc.Width, desc.Height))
    }

    /// Paint one egui frame over the backbuffer.
    ///
    /// The render target view is created and dropped inside this call — see the
    /// module docs for why that is not an optimisation opportunity.
    ///
    /// # Safety
    /// As [`Painter::new`].
    pub unsafe fn paint(
        &mut self,
        swapchain: &IDXGISwapChain,
        ctx: &egui::Context,
        output: RendererOutput,
    ) -> windows::core::Result<()> {
        // SAFETY: buffer 0 of a live swapchain is the current backbuffer.
        let backbuffer: ID3D11Texture2D = unsafe { swapchain.GetBuffer(0) }?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: valid out-param.
        unsafe { backbuffer.GetDesc(&raw mut desc) };

        let view_desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: gamma_view_format(desc.Format),
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 },
            },
        };
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        // SAFETY: `backbuffer` is a valid resource and `view_desc` describes a
        // 2D RTV over its mip 0 in a format compatible with its own.
        unsafe {
            self.device
                .CreateRenderTargetView(&backbuffer, Some(&raw const view_desc), Some(&raw mut rtv))
        }?;
        let Some(rtv) = rtv else {
            return Ok(());
        };

        {
            // Restored when this guard drops — including on the `?` below.
            // SAFETY: our immediate context, on the render thread.
            let _restore = unsafe { PipelineState::save(&self.context) };
            self.renderer.render(&self.context, &rtv, ctx, output)?;
        }

        // The guard has already rebound the game's targets; dropping the view
        // here leaves nothing of ours holding the backbuffer.
        drop(rtv);
        Ok(())
    }
}

/// The window this swapchain presents into.
///
/// The one window the game is definitely using, which is what input hooking
/// needs — enumerating the process's windows and picking one is a guess.
///
/// # Safety
/// `swapchain` must be the live `IDXGISwapChain` from the present callback.
pub unsafe fn output_window(swapchain: &IDXGISwapChain) -> Option<isize> {
    // SAFETY: a live swapchain always has a description.
    let desc = unsafe { swapchain.GetDesc() }.ok()?;
    (!desc.OutputWindow.0.is_null()).then_some(desc.OutputWindow.0 as isize)
}

/// Map an sRGB-aware backbuffer format to its plain gamma-space twin.
///
/// egui does its blending in gamma space and the renderer requires a
/// non-sRGB-aware view; anything already non-sRGB passes through untouched.
#[must_use]
pub fn gamma_view_format(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_FORMAT_B8G8R8X8_UNORM_SRGB => DXGI_FORMAT_B8G8R8X8_UNORM,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_formats_are_downgraded_for_gamma_blending() {
        assert_eq!(
            gamma_view_format(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB),
            DXGI_FORMAT_R8G8B8A8_UNORM
        );
        assert_eq!(
            gamma_view_format(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB),
            DXGI_FORMAT_B8G8R8A8_UNORM
        );
    }

    #[test]
    fn non_srgb_formats_pass_through() {
        assert_eq!(
            gamma_view_format(DXGI_FORMAT_R8G8B8A8_UNORM),
            DXGI_FORMAT_R8G8B8A8_UNORM
        );
    }
}
