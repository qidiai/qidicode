"""PDF operations CLI tool. Usage: python pdf_ops.py <command> [args]"""
import sys
import os
import json
import argparse

def cmd_extract_text(args):
    import pdfplumber
    pages_text = []
    with pdfplumber.open(args.input) as pdf:
        for i, page in enumerate(pdf.pages):
            text = page.extract_text() or ""
            pages_text.append({"page": i + 1, "text": text})
    if args.json:
        print(json.dumps(pages_text, ensure_ascii=False, indent=2))
    else:
        for p in pages_text:
            print(f"--- Page {p['page']} ---")
            print(p["text"])
    if not pages_text:
        print("Warning: No text extracted. PDF may be scanned images (OCR needed).")

def cmd_merge(args):
    from pypdf import PdfWriter
    writer = PdfWriter()
    for f in args.inputs:
        writer.append(f)
    writer.write(args.output)
    writer.close()
    print(f"Success: Merged {len(args.inputs)} files -> {args.output}")

def cmd_split(args):
    from pypdf import PdfReader, PdfWriter
    reader = PdfReader(args.input)
    total = len(reader.pages)
    base = os.path.splitext(args.input)[0]
    count = 0
    if args.range:
        start, end = parse_range(args.range, total)
        writer = PdfWriter()
        for i in range(start - 1, end):
            writer.add_page(reader.pages[i])
        out = f"{base}_p{start}-{end}.pdf"
        writer.write(out)
        writer.close()
        print(f"Success: Extracted pages {start}-{end} -> {out}")
        count = 1
    else:
        for i in range(total):
            writer = PdfWriter()
            writer.add_page(reader.pages[i])
            out = f"{base}_p{i+1}.pdf"
            writer.write(out)
            writer.close()
            count += 1
        print(f"Success: Split {total} pages -> {count} files (prefix: {base}_p*.pdf)")

def cmd_rotate(args):
    from pypdf import PdfReader, PdfWriter
    reader = PdfReader(args.input)
    writer = PdfWriter()
    angle = args.angle
    for page in reader.pages:
        page.rotate(angle)
        writer.add_page(page)
    writer.write(args.output)
    writer.close()
    print(f"Success: Rotated {len(reader.pages)} pages by {angle}° -> {args.output}")

def cmd_info(args):
    from pypdf import PdfReader
    reader = PdfReader(args.input)
    meta = reader.metadata
    info = {
        "pages": len(reader.pages),
        "metadata": {k: str(v) for k, v in meta.items()} if meta else {},
        "encrypted": reader.is_encrypted,
    }
    print(json.dumps(info, ensure_ascii=False, indent=2))

def parse_range(r, total):
    parts = r.split("-")
    start = max(1, int(parts[0]))
    end = min(total, int(parts[1])) if len(parts) > 1 else start
    return start, end

def main():
    parser = argparse.ArgumentParser(description="PDF operations tool")
    sub = parser.add_subparsers(dest="command", required=True)

    p_extract = sub.add_parser("extract-text", help="Extract text from PDF")
    p_extract.add_argument("input", help="Input PDF file")
    p_extract.add_argument("--json", action="store_true", help="Output as JSON")

    p_merge = sub.add_parser("merge", help="Merge multiple PDFs")
    p_merge.add_argument("inputs", nargs="+", help="Input PDF files")
    p_merge.add_argument("-o", "--output", required=True, help="Output PDF file")

    p_split = sub.add_parser("split", help="Split PDF into pages")
    p_split.add_argument("input", help="Input PDF file")
    p_split.add_argument("--range", help="Page range e.g. 1-5 (default: all pages)")

    p_rotate = sub.add_parser("rotate", help="Rotate PDF pages")
    p_rotate.add_argument("input", help="Input PDF file")
    p_rotate.add_argument("angle", type=int, choices=[90, 180, 270], help="Rotation angle")
    p_rotate.add_argument("-o", "--output", required=True, help="Output PDF file")

    p_info = sub.add_parser("info", help="Get PDF metadata")
    p_info.add_argument("input", help="Input PDF file")

    args = parser.parse_args()
    commands = {
        "extract-text": cmd_extract_text,
        "merge": cmd_merge,
        "split": cmd_split,
        "rotate": cmd_rotate,
        "info": cmd_info,
    }
    try:
        commands[args.command](args)
    except FileNotFoundError as e:
        print(f"Error: File not found: {e.filename}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
