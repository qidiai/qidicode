#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""t2_wps_child.py — WPS COM 导出子进程（T2 规格书 Part A.3）。

隔离设计（本仓库 8-22 事故铁律：COM 只在本独立进程，父进程绝不 Dispatch）：
  - 子进程 Dispatch('KWPS.Application')，Visible=False
  - Documents.Open(path, ReadOnly=True) → ExportAsFixedFormat(pdf, 17) → Close(False)
  - 只读打开，绝不 Save；每份开始/结束 append 一行带时间戳进度到 --log
  - 父进程(t2_run.py)通过读取该 log 做单份 300s 看门狗；本进程不自杀

用法:
  python t2_wps_child.py --jobs <jobs.json> --log <log.txt> [--root <out/t2/pdf>]

jobs.json: {"root": "...", "jobs": [{"idx": 1, "docx": "...", "pdf": "..."}, ...]}

退出码: 0=全部处理(含个别 FAIL)；3=Dispatch 失败(写 UNAVAILABLE)；2=参数/文件错误
参考（只读，不 import）：G:\\程序开发\\标书软件\\services\\doc_exporter.py:1095
"""
import argparse
import io
import json
import os
import sys
import time
from datetime import datetime

# 导出格式常量（与参考实现一致：wdExportFormatPDF）
WD_EXPORT_FORMAT_PDF = 17


def setup_stdout():
    try:
        if getattr(sys.stdout, "_craft_utf8", False):
            return
        if sys.stdout is not None and hasattr(sys.stdout, "buffer"):
            w = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
            w._craft_utf8 = True
            sys.stdout = w
    except Exception:
        pass


def _ts():
    return datetime.now().isoformat(timespec="seconds")


class Logger:
    """append-only 进度日志（每行 flush，父进程据此看门狗）。"""

    def __init__(self, path):
        self.path = path
        d = os.path.dirname(os.path.abspath(path))
        if d and not os.path.isdir(d):
            os.makedirs(d, exist_ok=True)
        self.fh = open(path, "a", encoding="utf-8")

    def line(self, msg):
        self.fh.write("%s %s\n" % (_ts(), msg))
        self.fh.flush()
        try:
            os.fsync(self.fh.fileno())
        except OSError:
            pass

    def close(self):
        try:
            self.fh.close()
        except Exception:
            pass


def assert_under(path, root):
    """写路径断言：path 必须落在 root 下。"""
    root_abs = os.path.abspath(root)
    ap = os.path.abspath(path)
    if not (ap == root_abs or ap.startswith(root_abs + os.sep)):
        raise AssertionError("write outside %s blocked: %s" % (root, path))


def main(argv=None):
    setup_stdout()
    ap = argparse.ArgumentParser(prog="t2_wps_child.py")
    ap.add_argument("--jobs", required=True, help="jobs.json 路径")
    ap.add_argument("--log", required=True, help="进度日志路径")
    args = ap.parse_args(argv)

    with open(args.jobs, "r", encoding="utf-8") as fh:
        spec = json.load(fh)
    jobs = spec.get("jobs", [])
    root = spec.get("root")
    if not root:
        print("jobs.json 缺 root", file=sys.stderr)
        return 2

    # 输出断言：all pdf under root，日志亦须落在 root 下
    for j in jobs:
        assert_under(j["pdf"], root)
    assert_under(args.log, root)

    log = Logger(args.log)
    log.line("INIT pid=%d njobs=%d" % (os.getpid(), len(jobs)))

    try:
        import pythoncom
        import win32com.client
        pythoncom.CoInitialize()
    except Exception as e:
        log.line("UNAVAILABLE ts=%s err=%s: %s" % (_ts(), type(e).__name__, e))
        log.close()
        return 3

    try:
        app = win32com.client.Dispatch("KWPS.Application")
    except Exception as e:
        log.line("UNAVAILABLE ts=%s err=%s: %s" % (_ts(), type(e).__name__, e))
        log.close()
        return 3

    try:
        try:
            app.Visible = False
        except Exception:
            pass
        log.line("READY ts=%s prog=KWPS.Application" % _ts())

        for j in jobs:
            idx = j["idx"]
            docx = os.path.abspath(j["docx"])
            pdf = os.path.abspath(j["pdf"])
            log.line("START idx=%s ts=%s" % (idx, _ts()))
            doc = None
            try:
                doc = app.Documents.Open(docx, ReadOnly=True)
                try:
                    doc.ExportAsFixedFormat(pdf, WD_EXPORT_FORMAT_PDF)
                finally:
                    try:
                        doc.Close(False)  # 绝不保存
                    except Exception:
                        pass
                log.line("DONE idx=%s ts=%s bytes=%d" % (
                    idx, _ts(), os.path.getsize(pdf) if os.path.exists(pdf) else -1))
            except Exception as e:
                log.line("FAIL idx=%s ts=%s err=%s: %s" % (
                    idx, _ts(), type(e).__name__, e))
    finally:
        log.line("EXIT ts=%s" % _ts())
        try:
            app.Quit()
        except Exception:
            pass
        try:
            pythoncom.CoUninitialize()
        except Exception:
            pass
        log.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
