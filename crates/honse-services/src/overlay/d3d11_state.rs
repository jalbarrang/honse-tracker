//! DX11 pipeline-state backup/restore bracketing the overlay render pass.
//!
//! `egui_directx11::Renderer::render` sets its own IA/VS/PS/RS/OM state and does
//! not restore what was bound before (its docs tell callers to back up). Edge
//! ships an equivalent `d3d11_backup.rs` for the same reason. The backup covers
//! exactly the stages the egui renderer touches, lives only within a single
//! Present (so no backbuffer-derived reference survives the frame — FINDINGS #1),
//! and restores on drop of the captured COM references.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState, ID3D11DepthStencilView, ID3D11DeviceContext,
    ID3D11GeometryShader, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
    ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11VertexShader, D3D11_VIEWPORT,
    D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

const MAX_RECTS: usize = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE as usize;

/// Snapshot of the pipeline state the egui renderer clobbers.
pub(super) struct StateBackup {
    scissor_rects: [RECT; MAX_RECTS],
    scissor_count: u32,
    viewports: [D3D11_VIEWPORT; MAX_RECTS],
    viewport_count: u32,
    rasterizer: Option<ID3D11RasterizerState>,
    blend_state: Option<ID3D11BlendState>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    depth_stencil: Option<ID3D11DepthStencilState>,
    stencil_ref: u32,
    render_targets: [Option<ID3D11RenderTargetView>; 1],
    depth_stencil_view: Option<ID3D11DepthStencilView>,
    ps_srv: [Option<ID3D11ShaderResourceView>; 1],
    ps_sampler: [Option<ID3D11SamplerState>; 1],
    pixel_shader: Option<ID3D11PixelShader>,
    vertex_shader: Option<ID3D11VertexShader>,
    geometry_shader: Option<ID3D11GeometryShader>,
    vs_constant_buffers: [Option<ID3D11Buffer>; 1],
    topology: D3D_PRIMITIVE_TOPOLOGY,
    index_buffer: Option<ID3D11Buffer>,
    index_format: DXGI_FORMAT,
    index_offset: u32,
    vertex_buffer: Option<ID3D11Buffer>,
    vertex_stride: u32,
    vertex_offset: u32,
    input_layout: Option<ID3D11InputLayout>,
}

impl StateBackup {
    /// Capture the current pipeline state (refcounts bumped by the getters; all
    /// references are released when the backup drops at end of frame).
    pub(super) fn capture(ctx: &ID3D11DeviceContext) -> Self {
        let mut backup = Self {
            scissor_rects: [RECT::default(); MAX_RECTS],
            scissor_count: MAX_RECTS as u32,
            viewports: [D3D11_VIEWPORT::default(); MAX_RECTS],
            viewport_count: MAX_RECTS as u32,
            rasterizer: None,
            blend_state: None,
            blend_factor: [0.0; 4],
            sample_mask: 0,
            depth_stencil: None,
            stencil_ref: 0,
            render_targets: [None],
            depth_stencil_view: None,
            ps_srv: [None],
            ps_sampler: [None],
            pixel_shader: None,
            vertex_shader: None,
            geometry_shader: None,
            vs_constant_buffers: [None],
            topology: D3D_PRIMITIVE_TOPOLOGY::default(),
            index_buffer: None,
            index_format: DXGI_FORMAT::default(),
            index_offset: 0,
            vertex_buffer: None,
            vertex_stride: 0,
            vertex_offset: 0,
            input_layout: None,
        };
        // SAFETY: live immediate context; every call writes only into the
        // matching out-params sized per the API contract.
        unsafe {
            ctx.RSGetScissorRects(&mut backup.scissor_count, Some(backup.scissor_rects.as_mut_ptr()));
            ctx.RSGetViewports(&mut backup.viewport_count, Some(backup.viewports.as_mut_ptr()));
            backup.rasterizer = ctx.RSGetState().ok();
            ctx.OMGetBlendState(
                Some(&mut backup.blend_state),
                Some(&mut backup.blend_factor),
                Some(&mut backup.sample_mask),
            );
            ctx.OMGetDepthStencilState(Some(&mut backup.depth_stencil), Some(&mut backup.stencil_ref));
            ctx.OMGetRenderTargets(Some(&mut backup.render_targets), Some(&mut backup.depth_stencil_view));
            ctx.PSGetShaderResources(0, Some(&mut backup.ps_srv));
            ctx.PSGetSamplers(0, Some(&mut backup.ps_sampler));
            ctx.PSGetShader(&mut backup.pixel_shader, None, None);
            ctx.VSGetShader(&mut backup.vertex_shader, None, None);
            ctx.GSGetShader(&mut backup.geometry_shader, None, None);
            ctx.VSGetConstantBuffers(0, Some(&mut backup.vs_constant_buffers));
            backup.topology = ctx.IAGetPrimitiveTopology();
            ctx.IAGetIndexBuffer(
                Some(&mut backup.index_buffer),
                Some(&mut backup.index_format),
                Some(&mut backup.index_offset),
            );
            ctx.IAGetVertexBuffers(
                0,
                1,
                Some(&mut backup.vertex_buffer),
                Some(&mut backup.vertex_stride),
                Some(&mut backup.vertex_offset),
            );
            backup.input_layout = ctx.IAGetInputLayout().ok();
        }
        backup
    }

    /// Rebind the captured state. Called after the egui render pass, still
    /// inside the same Present.
    pub(super) fn restore(&self, ctx: &ID3D11DeviceContext) {
        // SAFETY: live immediate context; rebinding references captured this frame.
        unsafe {
            ctx.RSSetScissorRects(Some(&self.scissor_rects[..self.scissor_count as usize]));
            ctx.RSSetViewports(Some(&self.viewports[..self.viewport_count as usize]));
            ctx.RSSetState(self.rasterizer.as_ref());
            ctx.OMSetBlendState(self.blend_state.as_ref(), Some(&self.blend_factor), self.sample_mask);
            ctx.OMSetDepthStencilState(self.depth_stencil.as_ref(), self.stencil_ref);
            ctx.OMSetRenderTargets(Some(&self.render_targets), self.depth_stencil_view.as_ref());
            ctx.PSSetShaderResources(0, Some(&self.ps_srv));
            ctx.PSSetSamplers(0, Some(&self.ps_sampler));
            ctx.PSSetShader(self.pixel_shader.as_ref(), None);
            ctx.VSSetShader(self.vertex_shader.as_ref(), None);
            ctx.GSSetShader(self.geometry_shader.as_ref(), None);
            ctx.VSSetConstantBuffers(0, Some(&self.vs_constant_buffers));
            ctx.IASetPrimitiveTopology(self.topology);
            ctx.IASetIndexBuffer(self.index_buffer.as_ref(), self.index_format, self.index_offset);
            ctx.IASetVertexBuffers(
                0,
                1,
                Some(&self.vertex_buffer as *const Option<ID3D11Buffer>),
                Some(&self.vertex_stride),
                Some(&self.vertex_offset),
            );
            ctx.IASetInputLayout(self.input_layout.as_ref());
        }
    }
}
