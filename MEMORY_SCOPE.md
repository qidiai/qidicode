# 记忆系统使用指南

## 快速启动

在 QIDI Code 中输入 `/m` 或 `/memory` 查看可用命令：

```
/m memory clear      # 清空记忆
/m memory scope isolated    # 完全隔离模式
/m memory scope project     # 项目共享模式
/m memory scope shared      # 全局共享模式
/m memory status          # 查看当前配置
```

## 三种记忆模式

| 命令 | 模式 | 使用场景 | 隔离级别 |
|------|------|---------|---------|
| `/m scope isolated` | 隔离模式 | 研究新项目，避免干扰 | 完全隔离 |
| `/m scope project` | 项目模式 | 团队协作开发同一项目 | 项目级共享 |
| `/m scope shared` | 共享模式 | 知识沉淀和团队共享 | 全局共享 |

## 使用示例

### 场景：多实例协作研究

**实例 1 - 研究新 Rust async 最佳实践：**
```
> /m scope isolated
> 记住这个 Rust async 模式...
> /m memory clear --workspace  # 清理临时研究笔记
```

**实例 2 - 使用共享知识：**
```
> /m scope shared
> /sync-pull techniques/async-patterns
```

**实例 3 - 继续其他研究（保持隔离）：**
```
> /m scope isolated
> 研究新的 AI 架构...
```

## 配置文件

编辑 `~/.qidi/config.toml` 设置默认模式：

```toml
[memory]
enabled = true
scope = "isolated"  # 默认模式
```

## 环境变量（临时切换）

```powershell
$env:QIDI_MEMORY_SCOPE = "project"
qidicode  # 启动时使用项目模式
```

## 常用命令速查

```
/m                    # 打开记忆管理菜单
/m status            # 查看当前记忆配置
/m scope isolated    # 切换到隔离模式
/m scope project     # 切换到项目模式
/m scope shared      # 切换到共享模式
/m clear             # 清空工作空间记忆
/m clear --global    # 清空全局记忆
/m clear --all       # 清空所有记忆
```

## 目录结构

```
~/.qidi/memory/
├── MEMORY.md                    # 全局记忆（shared 模式）
├── {project-hash}/             # 项目记忆（project 模式）
│   └── MEMORY.md
├── instance-{pid}/             # 实例记忆（isolated 模式）
│   └── MEMORY.md
└── shared/                     # 共享池
    ├── MEMORY.md
    └── techniques/             # 技术发现
        └── {timestamp}.md
```

## 注意事项

- 切换模式后，当前会话的搜索范围会立即改变
- 记忆索引需要在下次启动时重新加载
- 建议使用 `isolated` 模式进行探索性研究
- 使用 `shared` 模式进行长期知识沉淀
