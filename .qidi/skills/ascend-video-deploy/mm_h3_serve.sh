#!/usr/bin/env bash
# =============================================================================
# MiniMax H3 INT8 · 昇腾容器一键部署/会话恢复  (v3 实战版, 2026-09-18 全流程验证)
#
# 用法:
#   bash mm_h3_serve.sh              # 全流程 (首次 ~35 分钟 / 会话恢复 ~25 分钟)
#
# 实战验证环境 (GitCode Notebook):
#   - 1× Ascend 910B4 32G HBM, 16vCPU/64GB, CANN 8.5, python 3.11.14, 非 root
#   - 会话 2 小时上限, /opt/atomgit 存储不跨会话持久 (每次重跑本脚本即重建)
#   - 双卡环境自动切换双卡 turbo 工作流
#
# 本脚本吸收的实战坑 (每条都在真实会话中踩过并修复):
#   P1  仓库脚本硬编码 /data 且非 root 不可建软链 → sed 适配 WORK_ROOT
#   P2  submodule 是 SSH url 无 key 拉不动 → 覆写 HTTPS
#   P3  download_chunked.sh 的 mktemp 需要权重子目录先存在 → 预创建
#   P4  单连接 wget 仅 7.5MB/s (全量 121 分钟超会话限时) → 分片并行 ~85MB/s
#   P5  curl 段可能挂起 (最后 4G 停滞 10 分钟) → 段级检查+补下+合并+SHA 校验
#   P6  setup_comfyui.sh 依赖 uv 且 uv 不在 PATH → pip install uv + export PATH
#   P7  Notebook 镜像无 torchaudio → --no-deps 安装 (防 torch 被换成 CUDA 版)
#   P8  单卡放不下 27G 文本编码器 + 21G 扩散 → 文本编码器走 CPU + turbo 8 步
# =============================================================================
set -uo pipefail

# ---------- 可调参数 ----------
WORK_ROOT="${WORK_ROOT:-}"                 # 留空则自动探测
REPO_URL="https://gitcode.com/xujiashuai/minimax-h3-int8.git"
SUBMODULE_URL="https://gitcode.com/xujiashuai/ComfyUI.git"
REPO_NAME="mm"                             # 短目录名, 减少路径长度
PORT=8188
TSINGHUA="https://pypi.tuna.tsinghua.edu.cn/simple"
ASCEND_SET_ENV="${ASCEND_SET_ENV:-/usr/local/Ascend/ascend-toolkit/set_env.sh}"
# 5 个权重: 相对路径|SHA256|字节数|分片数
WEIGHTS=(
  "diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors|e889202c41dafb67b10d67b97f0d8541508036a6090af23425a5c2615d03c47a|20970379616|16"
  "text_encoders/qwen3vl_32b_minimax_h3_int8_convrot.safetensors|bc2ced0fbea64757fa9acddccfc0b3f4819d1dcf1da6c124d690d368be283923|27141342152|16"
  "vae/minimax_h3_video_vae_fp16.safetensors|7c1f131492e7eddacaac9069a61b81bdd39de5cc96561e677c5eab1cdce5e522|5207808496|8"
  "vae/minimax_h3_audio_vae_fp32.safetensors|8e505d95dd1561d47abd43d4238fd40d9bb1ae9e147ed0a4cba778d76ae4db48|605254808|4"
  "loras/minimax_h3_fl2v_turbo_8step_v1.0_comfyui_bf16.safetensors|2339acdf19bfe123f46b971ea35d367a84adb85de43627e1eceafa5a5b2b111e|1956193000|4"
)
BASE_URL="https://hf-mirror.com/Comfy-Org/MiniMax-H3/resolve/main"

log() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
die() { printf '\n\033[1;31m[中止] %s\033[0m\n' "$*" >&2; exit 1; }
ok()  { printf '    \033[1;32mOK\033[0m %s\n' "$*"; }

# ---------- [0/9] 工作目录探测 + Ascend 自检 ----------
log "[0/9] 环境自检"
if [ -z "$WORK_ROOT" ]; then
  if mkdir -p /data 2>/dev/null && [ -w /data ]; then WORK_ROOT=/data
  elif [ -d /opt/atomgit ] && [ -w /opt/atomgit ]; then WORK_ROOT=/opt/atomgit/data
  else die "找不到可写大目录: 试 WORK_ROOT=/path bash $0"
  fi
fi
mkdir -p "$WORK_ROOT"
REPO_DIR="$WORK_ROOT/$REPO_NAME"
WEIGHT_DIR="$WORK_ROOT/minimax_h3_int8"
export MODEL_ROOT="$WEIGHT_DIR"
echo "    工作根: $WORK_ROOT"

# shellcheck disable=SC1090
[ -f "$ASCEND_SET_ENV" ] && . "$ASCEND_SET_ENV"
npu_total="$(npu-smi info 2>/dev/null | grep -cE '910' || true)"
[ "${npu_total:-0}" -ge 1 ] || die "未检测到 Ascend 910 NPU"
ok "检测到 ${npu_total} 张 NPU"
python3 -c "import torch_npu" 2>/dev/null || die "import torch_npu 失败, 先 source $ASCEND_SET_ENV"
ok "torch_npu 可导入"
[ "$npu_total" -eq 1 ] && echo "    [单卡模式] 文本编码器走 CPU, 扩散模型走 NPU (turbo 8 步)"

# ---------- [1/9] 仓库 + submodule + 路径适配 (P1/P2) ----------
log "[1/9] 仓库"
if [ ! -d "$REPO_DIR/.git" ]; then
  git clone -q "$REPO_URL" "$REPO_DIR" || die "git clone 失败"
fi
cd "$REPO_DIR" || die "cd 失败"
# 仓库脚本里的 /data 全部改写为实际 WORK_ROOT (幂等: 已改过则跳过)
if grep -q 'DEST_ROOT="/data' scripts/download_minimax_weights.sh 2>/dev/null; then
  sed -i "s|/data/|$WORK_ROOT/|g" scripts/*.sh
  ok "脚本路径已适配 $WORK_ROOT"
fi
if [ ! -d "$REPO_DIR/third_party/ComfyUI/comfy" ]; then
  git config "submodule.third_party/ComfyUI.url" "$SUBMODULE_URL"
  git submodule update --init --depth 1 || die "submodule 拉取失败"
fi
ok "ComfyUI submodule 就绪"

# ---------- [2/9] 权重目录预创建 (P3) ----------
log "[2/9] 权重目录"
mkdir -p "$WEIGHT_DIR/diffusion_models" "$WEIGHT_DIR/text_encoders" \
         "$WEIGHT_DIR/vae" "$WEIGHT_DIR/loras"
ok "$WEIGHT_DIR"

# ---------- [3/9] 权重下载: 完整性检查 → 缺则分片并行 → 段级续传 (P4/P5) ----------
log "[3/9] 权重 (53GB, 分片并行)"
sha_of() { sha256sum "$1" | awk '{print $1}'; }

# 单文件分片下载 + 段级断点续传 + 合并校验 (实战验证的核心函数)
fetch_weight() {
  local rel="$1" sha="$2" total="$3" nch="$4"
  local dest="$WEIGHT_DIR/$rel"
  local dir; dir="$(dirname "$dest")"; mkdir -p "$dir"
  # 已有且校验通过 → 跳过
  if [ -s "$dest" ] && [ "$(sha_of "$dest")" = "$sha" ]; then
    ok "已有 $rel"; return 0
  fi
  local parts="$WEIGHT_DIR/parts_$(echo "$rel" | tr '/' '_')"
  mkdir -p "$parts"
  local ch=$(( (total + nch - 1) / nch ))
  # 段级补下: 只下缺失/不足的段 (P5 实战逻辑)
  local pids=() i s e ex p act
  for ((i=0; i<nch; i++)); do
    s=$(( i*ch )); e=$(( s+ch-1 )); [ $e -ge $total ] && e=$(( total-1 ))
    ex=$(( e-s+1 ))
    p="$(printf '%s/part.%04d' "$parts" "$i")"
    act=0; [ -f "$p" ] && act=$(stat -c%s "$p")
    if [ "$act" != "$ex" ]; then
      curl -sL --retry 5 --retry-delay 3 --connect-timeout 30 \
        -r "$s-$e" -o "$p" "$BASE_URL/$rel" &
      pids+=($!)
    fi
  done
  for pid in "${pids[@]}"; do wait "$pid" || true; done
  # 复查: 仍有不足段 → 再补一轮 (最多 3 轮)
  local round
  for round in 1 2 3; do
    local bad=0; pids=()
    for ((i=0; i<nch; i++)); do
      s=$(( i*ch )); e=$(( s+ch-1 )); [ $e -ge $total ] && e=$(( total-1 ))
      ex=$(( e-s+1 ))
      p="$(printf '%s/part.%04d' "$parts" "$i")"
      act=0; [ -f "$p" ] && act=$(stat -c%s "$p")
      if [ "$act" != "$ex" ]; then
        bad=$((bad+1))
        curl -sL --retry 5 --retry-delay 3 -r "$s-$e" -o "$p" "$BASE_URL/$rel" &
        pids+=($!)
      fi
    done
    [ $bad -eq 0 ] && break
    for pid in "${pids[@]}"; do wait "$pid" || true; done
  done
  # 合并 + 校验
  local f
  for f in "$parts"/part.*; do
    act=$(stat -c%s "$f")
    ch_check=$(( ch )); last=$(( total - (nch-1)*ch ))
    if [ "$f" = "$(printf '%s/part.%04d' "$parts" $((nch-1)))" ]; then ch_check=$last; fi
    [ "$act" = "$ch_check" ] || { echo "[段失败] $f ($act/$ch_check)"; return 1; }
  done
  cat "$parts"/part.* > "$dest"
  if [ "$(sha_of "$dest")" = "$sha" ]; then
    ok "$rel SHA-256 通过"; rm -rf "$parts"; return 0
  fi
  echo "[校验失败] $rel"; rm -f "$dest"; return 1
}

# 三路并行: 大文件×2 各一路, 小文件串行一路
wpids=()
( IFS='|' read -r R S T N <<< "${WEIGHTS[0]}"; fetch_weight "$R" "$S" "$T" "$N" ) & wpids+=($!)
( IFS='|' read -r R S T N <<< "${WEIGHTS[1]}"; fetch_weight "$R" "$S" "$T" "$N" ) & wpids+=($!)
( for wi in 2 3 4; do IFS='|' read -r R S T N <<< "${WEIGHTS[$wi]}"; fetch_weight "$R" "$S" "$T" "$N" || exit 1; done ) & wpids+=($!)
wfail=0
for wp in "${wpids[@]}"; do wait "$wp" || wfail=1; done
# 复核: 5 个文件存在且字节数精确匹配 (SHA 已在合并时校验)
for wline in "${WEIGHTS[@]}"; do
  IFS='|' read -r R S T N <<< "$wline"
  wact=0; [ -f "$WEIGHT_DIR/$R" ] && wact=$(stat -c%s "$WEIGHT_DIR/$R")
  [ "$wact" = "$T" ] || { echo "[缺失/损坏] $R"; wfail=1; }
done
[ $wfail -eq 0 ] || die "部分权重未就位, 重跑本脚本自动续传"
ok "5 个权重全部就位"

# ---------- [4/9] uv + ComfyUI venv + NPU patch + 软链 (P6) ----------
log "[4/9] ComfyUI 运行环境"
command -v uv >/dev/null 2>&1 || pip install -q -i "$TSINGHUA" uv
export PATH="$HOME/.local/bin:/usr/local/python3.11.14/bin:$PATH"
command -v uv >/dev/null 2>&1 || die "uv 不可用"
if [ ! -d "$REPO_DIR/third_party/ComfyUI/.venv" ]; then
  ./scripts/setup_comfyui.sh || die "环境搭建失败"
else
  ./scripts/setup_comfyui.sh --patches || die "patch 恢复失败"
fi

# ---------- [5/9] torchaudio 修复 (P7) ----------
log "[5/9] torchaudio (Notebook 镜像缺失)"
VENV_PY="$REPO_DIR/third_party/ComfyUI/.venv/bin/python"
if ! "$VENV_PY" -c "import torchaudio" 2>/dev/null; then
  "$VENV_PY" -m pip install -q --no-deps -i "$TSINGHUA" torchaudio==2.9.0 \
    || die "torchaudio 安装失败"
  "$VENV_PY" -c "import torchaudio" 2>/dev/null || die "torchaudio 导入失败"
fi
ok "torchaudio 可导入"

# ---------- [6/9] 按卡数生成工作流 (P8) ----------
log "[6/9] 生成工作流"
if [ "$npu_total" -ge 2 ]; then
  cp -f workflows/api_t2v_int8_turbo.json workflows/active_t2v.json
  ok "双卡 turbo 工作流"
else
  python3 scripts/add_turbo_lora.py \
      workflows/api_t2v_int8_single.json workflows/active_t2v.json 8 \
    || die "单卡工作流生成失败"
  ok "单卡+turbo 工作流 (文本编码器 CPU)"
fi

# ---------- [7/9] mmgen.py ----------
log "[7/9] API 封装 mmgen.py"
cat > "$REPO_DIR/mmgen.py" <<'PYEOF'
#!/usr/bin/env python3
"""MiniMax H3 文生视频一条命令封装 (默认工作流 active_t2v.json)"""
import argparse, json, os, random, sys, time, urllib.parse, urllib.request, uuid

def http_json(url, payload=None, timeout=60):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data,
        headers={} if data is None else {'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())

def node_inputs(wf, ct):
    for n in wf.values():
        if n.get('class_type') == ct: return n.get('inputs')
    return None

def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser()
    ap.add_argument('prompt')
    ap.add_argument('--host', default='127.0.0.1')
    ap.add_argument('--port', type=int, default=8188)
    ap.add_argument('--workflow', default=os.path.join(here, 'workflows', 'active_t2v.json'))
    ap.add_argument('--width', type=int, default=736)
    ap.add_argument('--height', type=int, default=416)
    ap.add_argument('--length', type=int, default=124)
    ap.add_argument('--steps', type=int, default=8)
    ap.add_argument('--seed', type=int, default=None)
    ap.add_argument('--fps', type=float, default=24.0)
    ap.add_argument('--prefix', default='video/mmgen')
    ap.add_argument('--outdir', default='.')
    ap.add_argument('--timeout', type=float, default=2400)
    a = ap.parse_args()
    base = 'http://{}:{}'.format(a.host, a.port)
    seed = a.seed if a.seed is not None else random.randint(0, 2**31-1)
    with open(a.workflow, encoding='utf-8') as f: wf = json.load(f)
    for ct, up in [('MiniMaxH3ImageToVideo', dict(prompt=a.prompt, width=a.width,
                    height=a.height, length=a.length)),
                   ('RandomNoise', dict(noise_seed=seed)),
                   ('BasicScheduler', dict(steps=a.steps)),
                   ('CreateVideo', dict(fps=a.fps)),
                   ('SaveVideo', dict(filename_prefix=a.prefix))]:
        ins = node_inputs(wf, ct)
        if ins is None: sys.exit('[错误] 找不到节点 ' + ct)
        ins.update(up)
    unet = node_inputs(wf, 'UNETLoader')
    if unet is not None: unet['weight_dtype'] = 'default'  # A910 禁 FP8
    try: http_json(base + '/system_stats', timeout=10)
    except Exception as e:
        sys.exit('[错误] ComfyUI 未就绪 ({}): {}'.format(base, e))
    print('[服务] 就绪 | 提交中...')
    r = http_json(base + '/prompt', {'prompt': wf, 'client_id': 'mmgen-' + uuid.uuid4().hex[:8]})
    if 'error' in r: sys.exit('[拒绝] ' + str(r['error']))
    pid = r['prompt_id']
    print('[提交] {} seed={} {}x{} {}帧 {}步'.format(pid[:8], seed, a.width, a.height, a.length, a.steps))
    t0 = time.time(); entry = None
    while time.time() - t0 < a.timeout:
        time.sleep(5)
        try: h = http_json(base + '/history/' + pid, timeout=15)
        except Exception: continue
        entry = h.get(pid)
        if entry:
            st = entry.get('status', {})
            if st.get('status_str') == 'error': sys.exit('[失败] ' + str(st.get('messages')))
            if st.get('completed'): break
        print('\r[等待] {}m{:02d}s'.format(int(time.time()-t0)//60, int(time.time()-t0)%60), end='', flush=True)
    else:
        sys.exit('[超时] 查 /history/' + pid)
    print()
    files = []
    for out in (entry.get('outputs') or {}).values():
        for k in ('images', 'videos', 'gifs'):
            for f in out.get(k) or []:
                if f.get('type') == 'output': files.append(f)
    if not files: sys.exit('[警告] 无输出文件')
    os.makedirs(a.outdir, exist_ok=True)
    for f in files:
        q = urllib.parse.urlencode({'filename': f['filename'],
            'subfolder': f.get('subfolder',''), 'type': f.get('type','output')})
        local = os.path.join(a.outdir, os.path.basename(f['filename']))
        with urllib.request.urlopen(base + '/view?' + q, timeout=300) as r2, open(local,'wb') as fo:
            while True:
                c = r2.read(1<<20)
                if not c: break
                fo.write(c)
        print('[完成] {} (耗时 {}s)'.format(local, int(time.time()-t0)))

if __name__ == '__main__':
    main()
PYEOF
ok "$REPO_DIR/mmgen.py"

# ---------- [8/9] 启动服务 ----------
log "[8/9] 启动 ComfyUI (端口 $PORT)"
./scripts/start-comfyui-dual-npu.sh start || die "启动失败, 查 $REPO_DIR/third_party/ComfyUI/logs/comfyui.log"
ready=0
for _ in $(seq 1 60); do
  curl -sf "http://127.0.0.1:$PORT/system_stats" >/dev/null 2>&1 && { ready=1; break; }
  sleep 3
done
[ "$ready" = "1" ] || die "服务未就绪, 查日志"
ok "服务就绪 (system_stats 正常)"

# ---------- [9/9] 使用摘要 ----------
cat <<SUMMARY

============================================================
 MiniMax H3 文生视频服务就绪 · ${npu_total} 卡模式 · 端口 $PORT
============================================================
 [出片]  $VENV_PY $REPO_DIR/mmgen.py "提示词. Audio: 音效" --outdir $WORK_ROOT
 [参数]  --width 736 --height 416 --length 124 --steps 8 --seed N
 [提示]  首条为冷启动 (权重上卡, 实测 13 分钟); 之后每条 ~2 分钟
 [恢复]  会话过期重开后, 重跑 bash $0 即可 (自动跳过已就绪步骤)
============================================================
SUMMARY
