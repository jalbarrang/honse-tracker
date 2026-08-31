//! Save/restore of the D3D11 pipeline state the egui pass clobbers.
//!
//! `egui_directx11::Renderer::render` documents exactly what it overrides and
//! explicitly does *not* put any of it back: input assembler, vertex and pixel
//! shader, rasterizer state + viewport + scissor, PS slot 0 SRV and sampler,
//! and the output merger's render targets and blend state. We paint from inside
//! the game's `Present`, so anything we leave altered is inherited by whatever
//! the game does next.
//!
//! In practice the game rebinds most of this at the start of its own frame, so
//! omitting the backup usually looks fine — right up until it doesn't, on one
//! driver, in one scene. This is cheap insurance and it stays.
//!
//! Captured on [`PipelineState::save`] and put back by `Drop`, so an early
//! return or a panic in the paint path cannot skip the restore.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11BlendState, ID3D11Buffer, ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState,
    ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11VertexShader, D3D11_VIEWPORT,
    D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

/// Slots D3D11 allows for viewports and scissor rects.
const MAX_RECTS: usize = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE as usize;

/// A snapshot of every pipeline slot the egui pass touches.
pub struct PipelineState {
    ctx: ID3D11DeviceContext,

    topology: D3D_PRIMITIVE_TOPOLOGY,
    input_layout: Option<ID3D11InputLayout>,
    vertex_buffer: Option<ID3D11Buffer>,
    vertex_stride: u32,
    vertex_offset: u32,
    index_buffer: Option<ID3D11Buffer>,
    index_format: DXGI_FORMAT,
    index_offset: u32,

    vertex_shader: Option<ID3D11VertexShader>,
    pixel_shader: Option<ID3D11PixelShader>,
    pixel_srv: Option<ID3D11ShaderResourceView>,
    pixel_sampler: Option<ID3D11SamplerState>,

    rasterizer: Option<ID3D11RasterizerState>,
    viewports: Vec<D3D11_VIEWPORT>,
    scissors: Vec<RECT>,

    render_target: Option<ID3D11RenderTargetView>,
    blend: Option<ID3D11BlendState>,
    blend_factor: [f32; 4],
    blend_mask: u32,
}

impl PipelineState {
    /// Capture the current state of `ctx`.
    ///
    /// # Safety
    /// `ctx` must be the immediate context of the device that owns the
    /// swapchain we are about to paint into, called on the thread that owns it
    /// (the render thread, inside `Present`).
    pub unsafe fn save(ctx: &ID3D11DeviceContext) -> Self {
        // SAFETY: every call below is a plain getter on a live immediate
        // context; each out-param is sized per its D3D11 contract.
        unsafe {
            let topology = ctx.IAGetPrimitiveTopology();
            let input_layout = ctx.IAGetInputLayout().ok();

            let mut vertex_buffer = None;
            let mut vertex_stride = 0u32;
            let mut vertex_offset = 0u32;
            ctx.IAGetVertexBuffers(
                0,
                1,
                Some(&raw mut vertex_buffer),
                Some(&raw mut vertex_stride),
                Some(&raw mut vertex_offset),
            );

            let mut index_buffer = None;
            let mut index_format = DXGI_FORMAT::default();
            let mut index_offset = 0u32;
            ctx.IAGetIndexBuffer(
                Some(&raw mut index_buffer),
                Some(&raw mut index_format),
                Some(&raw mut index_offset),
            );

            let mut vertex_shader = None;
            ctx.VSGetShader(&raw mut vertex_shader, None, None);
            let mut pixel_shader = None;
            ctx.PSGetShader(&raw mut pixel_shader, None, None);

            let mut srv = [const { None }; 1];
            ctx.PSGetShaderResources(0, Some(&mut srv));
            let mut sampler = [const { None }; 1];
            ctx.PSGetSamplers(0, Some(&mut sampler));

            let rasterizer = ctx.RSGetState().ok();

            let mut viewport_count = MAX_RECTS as u32;
            let mut viewports = vec![D3D11_VIEWPORT::default(); MAX_RECTS];
            ctx.RSGetViewports(&raw mut viewport_count, Some(viewports.as_mut_ptr()));
            viewports.truncate(viewport_count as usize);

            let mut scissor_count = MAX_RECTS as u32;
            let mut scissors = vec![RECT::default(); MAX_RECTS];
            ctx.RSGetScissorRects(&raw mut scissor_count, Some(scissors.as_mut_ptr()));
            scissors.truncate(scissor_count as usize);

            let mut render_targets = [const { None }; 1];
            ctx.OMGetRenderTargets(Some(&mut render_targets), None);

            let mut blend = None;
            let mut blend_factor = [0.0f32; 4];
            let mut blend_mask = 0u32;
            ctx.OMGetBlendState(Some(&raw mut blend), Some(&mut blend_factor), Some(&raw mut blend_mask));

            Self {
                ctx: ctx.clone(),
                topology,
                input_layout,
                vertex_buffer,
                vertex_stride,
                vertex_offset,
                index_buffer,
                index_format,
                index_offset,
                vertex_shader,
                pixel_shader,
                pixel_srv: srv[0].take(),
                pixel_sampler: sampler[0].take(),
                rasterizer,
                viewports,
                scissors,
                render_target: render_targets[0].take(),
                blend,
                blend_factor,
                blend_mask,
            }
        }
    }
}

impl Drop for PipelineState {
    fn drop(&mut self) {
        let ctx = &self.ctx;
        // SAFETY: mirror of `save` on the same context and thread. Passing a
        // `None` interface unbinds the slot, which is the correct restore when
        // nothing was bound at save time.
        unsafe {
            ctx.IASetPrimitiveTopology(self.topology);
            ctx.IASetInputLayout(self.input_layout.as_ref());
            ctx.IASetVertexBuffers(
                0,
                1,
                Some(&raw const self.vertex_buffer),
                Some(&raw const self.vertex_stride),
                Some(&raw const self.vertex_offset),
            );
            ctx.IASetIndexBuffer(self.index_buffer.as_ref(), self.index_format, self.index_offset);

            ctx.VSSetShader(self.vertex_shader.as_ref(), None);
            ctx.PSSetShader(self.pixel_shader.as_ref(), None);
            ctx.PSSetShaderResources(0, Some(std::slice::from_ref(&self.pixel_srv)));
            ctx.PSSetSamplers(0, Some(std::slice::from_ref(&self.pixel_sampler)));

            ctx.RSSetState(self.rasterizer.as_ref());
            ctx.RSSetViewports(Some(&self.viewports));
            ctx.RSSetScissorRects(Some(&self.scissors));

            ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.render_target)), None);
            ctx.OMSetBlendState(self.blend.as_ref(), Some(&self.blend_factor), self.blend_mask);
        }
    }
}
