## 记忆系统范围控制

QIDI Code 支持三种记忆范围模式，通过环境变量或配置文件控制：

### 使用方法

#### 1. 配置文件（持久化）
编辑 `~/.qidi/config.toml`：
```toml
[memory]
enabled = true
scope = "isolated"  # 或 "project" / "shared"
```

#### 2. 环境变量（临时）
```powershell
# 隔离模式（默认）- 每个实例独立记忆
$env:QIDI_MEMORY_SCOPE = "isolated"
qidicode

# 项目模式 - 同仓库多实例共享
$env:QIDI_MEMORY_SCOPE = "project"
qidicode

# 共享模式 - 所有实例共享
$env:QIDI_MEMORY_SCOPE = "shared"
qidicode
```

### 三种模式说明

| 模式 | 使用场景 | 隔离级别 | 目录结构 |
|------|---------|---------|---------|
| `isolated` | 研究新项目，避免干扰 | 完全隔离 | `~/.qidi/memory/instance-{pid}/` |
| `project` | 团队协作开发同一项目 | 项目级共享 | `~/.qidi/memory/{project-hash}/` |
| `shared` | 知识沉淀和团队共享 | 全局共享 | `~/.qidi/memory/shared/` |

### 快速切换

```powershell
# 查看当前模式
qidicode --memory-scope

# 启动时指定
qidicode --memory-scope project
```
