# -*- coding: utf-8 -*-
"""truth.py — w:t XML 真值抽取 (T1 规格书 §2).

从 docx 的 word/document.xml 中抽取全部 <w:t> 文本作为真值；
剔除祖先为 <w:del> 的 <w:t>（旧式修订删除文本），剔除字符量计入 del_chars。
T2 Part C：del_chars 双计（另加全部 w:delText 的 len），新增 sym_count。
打不开/解析失败 → error 字段，不抛异常。
"""
import zipfile
import xml.etree.ElementTree as ET

from .metrics import norm

W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
W = "{%s}" % W_NS

DOCUMENT_XML = "word/document.xml"

# T2 Part C: w:delText 是独立标签（非 w:t 子集），w:sym 为符号元素
DEL_TEXT = W + "delText"
SYM = W + "sym"


def _collect(xml_bytes):
    """返回 (raw, del_chars, sym_count)。

    raw       : 非删除 <w:t> 文本按文档序拼接
    del_chars : 修订删除字符总数 = Σ(旧式 w:del 祖先下 w:t 的 len)
                + Σ(全部 w:delText 的 len)   [T2 Part C 双计]
    sym_count : w:sym 元素计数                   [T2 Part C 新增]
    """
    root = ET.fromstring(xml_bytes)

    # 建立子->父映射（ElementTree 无 parent 指针）。
    parent = {}
    for p in root.iter():
        for c in p:
            parent[c] = p

    parts = []
    del_chars = 0
    for el in root.iter(W + "t"):
        # 判断是否存在 <w:del> 祖先
        deleted = False
        node = el
        while node in parent:
            node = parent[node]
            if node.tag == W + "del":
                deleted = True
                break
        txt = el.text or ""
        if deleted:
            del_chars += len(txt)
        else:
            parts.append(txt)

    # 新式修订删除：w:delText 与 w:t 是不同标签，不被上面的迭代覆盖
    for el in root.iter(DEL_TEXT):
        del_chars += len(el.text or "")

    # 符号元素计数（w:sym 无文本节点，单独计数）
    sym_count = sum(1 for _ in root.iter(SYM))

    return "".join(parts), del_chars, sym_count


def extract_truth(docx_path):
    """抽取 docx 的 w:t 真值。

    T2 规格书 Part C: 返回 dict 增 `sym_count` 键（`del_chars` 语义修正为双计），
    其余键不动：`{"raw","norm_chars","del_chars","sym_count","error"}`。
    """
    res = {"raw": "", "norm_chars": 0, "del_chars": 0, "sym_count": 0, "error": None}

    try:
        with zipfile.ZipFile(docx_path) as z:
            xml_bytes = z.read(DOCUMENT_XML)
    except Exception as e:  # zip 打不开 / 无 document.xml
        res["error"] = "%s: %s" % (type(e).__name__, e)
        return res

    try:
        raw, del_chars, sym_count = _collect(xml_bytes)
    except Exception as e:  # XML 解析失败
        res["error"] = "%s: %s" % (type(e).__name__, e)
        return res

    res["raw"] = raw
    res["norm_chars"] = len(norm(raw))
    res["del_chars"] = del_chars
    res["sym_count"] = sym_count
    return res
