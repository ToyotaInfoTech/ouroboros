# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

import time

import wgpu

# === 設定 ===
N = 1 << 23
ITER = 1_000_000

# === アダプタ取得 ===
adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
device = adapter.request_device_sync()

print("Adapter:", adapter.info)

# === バッファ ===
buffer = device.create_buffer(
    size=N * 4,
    usage=wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC | wgpu.BufferUsage.COPY_DST,
)

# === シェーダ ===
shader_code = """
@group(0) @binding(0)
var<storage, read_write> data: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;

    var x0: f32 = f32(idx);
    var x1: f32 = f32(idx + 1u);
    var x2: f32 = f32(idx + 2u);
    var x3: f32 = f32(idx + 3u);

    for (var i: u32 = 0u; i < 50000u; i = i + 1u) {
        x0 = x0 * 1.000001 + 0.000001;
        x1 = x1 * 1.000002 + 0.000002;
        x2 = x2 * 1.000003 + 0.000003;
        x3 = x3 * 1.000004 + 0.000004;
    }

    data[idx] = x0 + x1 + x2 + x3;
}
"""

shader = device.create_shader_module(code=shader_code)

# === パイプライン ===
bind_group_layout = device.create_bind_group_layout(
    entries=[{
        "binding": 0,
        "visibility": wgpu.ShaderStage.COMPUTE,
        "buffer": {"type": wgpu.BufferBindingType.storage},
    }]
)

pipeline_layout = device.create_pipeline_layout(
    bind_group_layouts=[bind_group_layout]
)

pipeline = device.create_compute_pipeline(
    layout=pipeline_layout,
    compute={"module": shader, "entry_point": "main"},
)

bind_group = device.create_bind_group(
    layout=bind_group_layout,
    entries=[{
        "binding": 0,
        "resource": {"buffer": buffer},
    }],
)

# === 実行 ===
def run_compute():
    command_encoder = device.create_command_encoder()
    pass_enc = command_encoder.begin_compute_pass()
    pass_enc.set_pipeline(pipeline)
    pass_enc.set_bind_group(0, bind_group, [])
    workgroups = (N + 255) // 256
    pass_enc.dispatch_workgroups(workgroups, 1, 1)
    pass_enc.end()

    device.queue.submit([command_encoder.finish()])
    device.queue.on_submitted_work_done_sync()

# 空打ちしてウォームアップ
run_compute()

# タイム計測開始
start = time.time()
run_compute()
elapsed = time.time() - start

# === FLOPS計算 ===
flops = N * 50000 * 4 * 2  # 4レジスタ * 各ループ2FLOPs
tflops = flops / elapsed / 1e12

print(f"Time: {elapsed:.3f} sec")
print(f"Estimated TFLOPS: {tflops:.2f}")
