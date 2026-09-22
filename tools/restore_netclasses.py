#!/usr/bin/env python3
import argparse
from pathlib import Path


def blocks(text):
    result = []
    cursor = 0
    while True:
        start = text.find("\n\t(net_class", cursor)
        if start < 0:
            return result
        depth = 0
        quoted = False
        escaped = False
        end = len(text)
        for index in range(start + 1, len(text)):
            char = text[index]
            if quoted:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    quoted = False
            elif char == '"':
                quoted = True
            elif char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
                if depth == 0:
                    end = index + 1
                    break
        result.append(text[start:end])
        cursor = end


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source_board")
    parser.add_argument("output_board")
    args = parser.parse_args()
    source = Path(args.source_board).read_text()
    output = Path(args.output_board).read_text()
    if "\n\t(net_class" not in output:
        insertion = output.find("\n\t(gr_")
        if insertion < 0:
            insertion = output.find("\n\t(footprint")
        if insertion < 0:
            raise SystemExit("no safe netclass insertion point")
        output = output[:insertion] + "\n".join(blocks(source)) + output[insertion:]
    if "\n\t\t(trace_min " not in output:
        setup_marker = "\n\t\t(pad_to_mask_clearance"
        output = output.replace(
            setup_marker,
            "\n\t\t(last_trace_width 0.127)\n"
            "\t\t(trace_min 0.127)\n"
            "\t\t(clearance_min 0.127)" + setup_marker,
            1,
        )
    Path(args.output_board).write_text(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
