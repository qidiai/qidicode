# -*- coding: utf-8 -*-
"""truth.py — w:t XML 真值抽取 (T1 规格书 §2).

从 docx 的 word/document.xml 中抽取全部 <w:t> 文本作为真值；
剔除祖先为 <w:del> 的 <w:t>（修订删除文本），剔除字符量计入 del_chars。
打不开/解析失败 → error 字段，不抛异常。
"""
import zipfile
import xml.etree.ElementTree as ET

from .metrics import norm

W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
W = "{%s}" % W_NS

DOCUMENT_XML = "word/document.xml"


def _collect(xml_bytes):
    """返回 (raw, del_chars)。

    raw       : 非删除 <w:t> 文本按文档序拼接
    del_chars : 祖先为 <w:del> 的 <w:t> 文本字符总数
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

    return "".join(parts), del_chars


def extract_truth(docx_path):
    """抽取 docx 的 w:t 真值。

    规格书 §2: `extract_truth(docx_path) -> {"raw","norm_chars","del_chars","error"}`
    """
    res = {"raw": "", "norm_chars": 0, "del_chars": 0, "error": None}

    try:
        with zipfile.ZipFile(docx_path) as z:
            xml_bytes = z.read(DOCUMENT_XML)
    except Exception as e:  # zip 打不开 / 无 document.xml
        res["error"] = "%s: %s" % (type(e).__name__, e)
        return res

    try:
        raw, del_chars = _collect(xml_bytes)
    except Exception as e:  # XML 解析失败
        res["error"] = "%s: %s" % (type(e).__name__, e)
        return res

    res["raw"] = raw
    res["norm_chars"] = len(norm(raw))
    res["del_chars"] = del_chars
    return res
