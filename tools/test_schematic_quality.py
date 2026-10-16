#!/usr/bin/env python3
"""Run: python3 tools/test_schematic_quality.py

The score reads an exported schematic: wires from the `.kicad_sch`, text boxes
from KiCad's SVG plot of it (each string is a `stroked-text` group of glyph
paths). These tests feed it small hand-written files, so they do not need
kicad-cli.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import schematic_quality as sq  # noqa: E402


def sch(*wires, symbols=()):
    """A `.kicad_sch` with wires given as (x1, y1, x2, y2) and symbol anchors."""
    out = ["(kicad_sch"]
    for x1, y1, x2, y2 in wires:
        out.append(
            f"\t(wire\n\t\t(pts\n\t\t\t(xy {x1} {y1}) (xy {x2} {y2})\n\t\t)\n\t)"
        )
    for x, y in symbols:
        out.append(f'\t(symbol\n\t\t(lib_id "Device:R")\n\t\t(at {x} {y} 0)\n\t)')
    out.append(")")
    return "\n".join(out)


def svg(*texts, width=100.0, height=80.0):
    """A plot with `texts` given as (string, x0, y0, x1, y1) boxes."""
    out = [f'<svg viewBox="0.0000 0.0000 {width} {height}">']
    for s, x0, y0, x1, y1 in texts:
        out.append(
            f'<g class="stroked-text"><desc>{s}</desc>\n'
            f'<path d="M{x0} {y0}\nL{x1} {y0}\n" />\n'
            f'<path d="M{x0} {y1}\nL{x1} {y1}\n" />\n</g>'
        )
    out.append("</svg>")
    return "\n".join(out)


def score(sch_text, svg_text):
    return sq.score(
        sq.parse_wires(sch_text),
        sq.parse_texts(svg_text),
        sq.page_size(svg_text),
        sq.parse_content_points(sch_text),
        sq.parse_group_boxes(sch_text),
    )


class WireThroughText(unittest.TestCase):
    def test_a_wire_across_a_text_is_counted(self):
        got = score(sch((0, 5, 10, 5)), svg(("R1", 3, 4, 6, 6)))
        self.assertEqual(got["texts_with_a_wire_through_them"], 1)

    def test_a_text_clear_of_the_wire_is_not(self):
        got = score(sch((0, 5, 10, 5)), svg(("R1", 3, 7, 6, 9)))
        self.assertEqual(got["texts_with_a_wire_through_them"], 0)

    def test_a_wire_that_only_touches_the_text_edge_is_not(self):
        got = score(sch((0, 6, 10, 6)), svg(("R1", 3, 4, 6, 6)))
        self.assertEqual(got["texts_with_a_wire_through_them"], 0)


class TextOverlap(unittest.TestCase):
    def test_two_overlapping_texts_count_once(self):
        got = score(sch(), svg(("U1", 0, 0, 4, 2), ("MODEM_IDLE", 3, 1, 14, 3)))
        self.assertEqual(got["text_pairs_overlapping"], 1)

    def test_texts_side_by_side_do_not(self):
        got = score(sch(), svg(("U1", 0, 0, 4, 2), ("R1", 5, 0, 8, 2)))
        self.assertEqual(got["text_pairs_overlapping"], 0)

    def test_the_same_string_drawn_twice_in_one_place_is_not_a_collision(self):
        got = score(sch(), svg(("GND", 0, 0, 4, 2), ("GND", 0, 0, 4, 2)))
        self.assertEqual(got["text_pairs_overlapping"], 0)


class Duplicates(unittest.TestCase):
    def test_a_clash_with_a_doubled_string_counts_once(self):
        # KiCad draws "GND" twice here; the clash with U1 is still one clash.
        got = score(
            sch(),
            svg(("GND", 0, 0, 4, 2), ("GND", 0, 0, 4, 2), ("U1", 3, 1, 6, 3)),
        )
        self.assertEqual(got["text_pairs_overlapping"], 1)

    def test_a_doubled_string_with_a_wire_through_it_counts_once(self):
        got = score(
            sch((0, 5, 10, 5)),
            svg(("R1", 3, 4, 6, 6), ("R1", 3, 4, 6, 6)),
        )
        self.assertEqual(got["texts_with_a_wire_through_them"], 1)

    def test_the_same_string_in_two_places_is_two_texts(self):
        got = score(
            sch((0, 5, 10, 5), (0, 25, 10, 25)),
            svg(("R1", 3, 4, 6, 6), ("R1", 3, 24, 6, 26)),
        )
        self.assertEqual(got["texts_with_a_wire_through_them"], 2)


class WireAndTextShapes(unittest.TestCase):
    def test_a_wire_wholly_inside_a_text_box_is_through_it(self):
        got = score(sch((4, 5, 5, 5)), svg(("R1", 3, 4, 6, 6)))
        self.assertEqual(got["texts_with_a_wire_through_them"], 1)

    def test_overlapping_collinear_wires_do_not_cross(self):
        got = score(sch((0, 5, 10, 5), (5, 5, 15, 5)), svg())
        self.assertEqual(got["wire_crossings"], 0)


def sch_with_boxes(*boxes):
    """A `.kicad_sch` with group boxes as (x1, y1, x2, y2), plus one rectangle
    inside a symbol definition, which is not a group box."""
    out = ["(kicad_sch", "\t(lib_symbols", '\t\t(symbol "Device:R"',
           "\t\t\t(rectangle (start -1 -2) (end 1 2))", "\t\t)", "\t)"]
    for x1, y1, x2, y2 in boxes:
        out.append(f"\t(rectangle\n\t\t(start {x1} {y1})\n\t\t(end {x2} {y2})\n\t)")
    out.append(")")
    return "\n".join(out)


class RegionCoverage(unittest.TestCase):
    def cover(self, *boxes):
        return sq.region_coverage(sq.parse_group_boxes(sch_with_boxes(*boxes)))

    def test_boxes_that_tile_a_rectangle_cover_all_of_it(self):
        self.assertAlmostEqual(self.cover((0, 0, 10, 20), (10, 0, 30, 20)), 1.0)

    def test_a_short_box_beside_a_tall_one_leaves_a_hole(self):
        # Bounding rectangle 30 x 20 = 600; the boxes cover 200 + 200.
        self.assertAlmostEqual(self.cover((0, 0, 10, 20), (10, 0, 30, 10)), 400.0 / 600.0, places=4)

    def test_a_rectangle_inside_a_symbol_is_not_a_group_box(self):
        self.assertEqual(len(sq.parse_group_boxes(sch_with_boxes((0, 0, 10, 20)))), 1)

    def test_no_group_boxes_is_zero(self):
        self.assertEqual(self.cover(), 0.0)

    def test_score_carries_it(self):
        got = sq.score([], [], (100.0, 80.0), (), group_boxes=[(0, 0, 10, 20), (10, 0, 30, 10)])
        self.assertAlmostEqual(got["region_coverage"], 400.0 / 600.0, places=4)


class PickPlot(unittest.TestCase):
    def test_the_plot_named_after_the_schematic_wins(self):
        paths = [Path("a/sub.svg"), Path("a/board.svg")]
        self.assertEqual(sq.pick_plot(paths, "board"), Path("a/board.svg"))

    def test_a_single_plot_is_used_whatever_it_is_called(self):
        self.assertEqual(sq.pick_plot([Path("a/x.svg")], "board"), Path("a/x.svg"))

    def test_several_plots_and_none_for_the_root_is_an_error(self):
        with self.assertRaises(ValueError):
            sq.pick_plot([Path("a/one.svg"), Path("a/two.svg")], "board")


class WireCrossings(unittest.TestCase):
    def test_two_wires_that_cross_count_once(self):
        got = score(sch((0, 5, 10, 5), (5, 0, 5, 10)), svg())
        self.assertEqual(got["wire_crossings"], 1)

    def test_a_t_junction_is_not_a_crossing(self):
        got = score(sch((0, 5, 10, 5), (5, 5, 5, 10)), svg())
        self.assertEqual(got["wire_crossings"], 0)

    def test_parallel_wires_do_not_cross(self):
        got = score(sch((0, 5, 10, 5), (0, 7, 10, 7)), svg())
        self.assertEqual(got["wire_crossings"], 0)


class WireLengthAndFill(unittest.TestCase):
    def test_wire_length_is_the_sum_of_the_segments(self):
        got = score(sch((0, 0, 10, 0), (0, 0, 0, 5)), svg())
        self.assertEqual(got["wire_length_mm"], 15.0)

    def test_content_density_is_the_share_of_cells_in_use(self):
        # Two wires 20 mm apart span 3 x 3 cells of 10 mm; the wires run
        # through the 3 cells of the top row and the 3 of the bottom row.
        got = score(sch((0, 0, 20, 0), (0, 20, 20, 20)), svg())
        self.assertAlmostEqual(got["content_density"], 6.0 / 9.0, places=4)

    def test_a_text_fills_the_cells_it_covers(self):
        got = score(sch((0, 20, 0, 20.1), (20, 40, 20, 40.1)), svg(("R1", 0, 30, 14, 32)))
        # Cells 0..2 in x and y: the wires touch 2, the text 2 more.
        self.assertAlmostEqual(got["content_density"], 4.0 / 9.0, places=4)

    def test_the_page_ruler_and_title_block_do_not_stretch_the_content_box(self):
        # Content sits in x 20..40; a ruler letter at x 2 and title block text
        # at x 85 are far outside it and must not count.
        got = score(
            sch((20, 20, 40, 20), (20, 40, 40, 40), symbols=[(30, 30)]),
            svg(("A", 1, 30, 3, 32), ("Sheet", 85, 60, 95, 62)),
        )
        # Cells x 2..4, y 2..4: wires touch the top row (3) and bottom row (3),
        # the symbol the middle one: 7 of 9.
        self.assertAlmostEqual(got["content_density"], 7.0 / 9.0, places=4)

    def test_an_empty_schematic_scores_zero_everywhere(self):
        got = score(sch(), svg())
        self.assertEqual(
            (got["text_pairs_overlapping"], got["wire_crossings"], got["content_density"]),
            (0, 0, 0.0),
        )


def parts_sch(*placed):
    """A `.kicad_sch` with a two-pin passive symbol `Device:R` (pins at 0,+3.81
    and 0,-3.81) and an IC symbol `synth:U` (pins 1, 2 and 3 at 10,0, 10,-5 and 10,-10),
    and `placed` instances given as (lib_id, ref, x, y, angle, mirror)."""
    out = [
        "(kicad_sch",
        "\t(lib_symbols",
        '\t\t(symbol "Device:R"',
        '\t\t\t(symbol "R_1_1"',
        '\t\t\t\t(pin passive line\n\t\t\t\t\t(at 0 3.81 270)\n\t\t\t\t\t(number "1")\n\t\t\t\t)',
        '\t\t\t\t(pin passive line\n\t\t\t\t\t(at 0 -3.81 90)\n\t\t\t\t\t(number "2")\n\t\t\t\t)',
        "\t\t\t)",
        "\t\t)",
        '\t\t(symbol "Device:D"',
        '\t\t\t(symbol "D_1_1"',
        '\t\t\t\t(pin passive line\n\t\t\t\t\t(at 0 3.81 270)\n\t\t\t\t\t(number "1")\n\t\t\t\t)',
        '\t\t\t\t(pin passive line\n\t\t\t\t\t(at 0 -3.81 90)\n\t\t\t\t\t(number "2")\n\t\t\t\t)',
        "\t\t\t)",
        "\t\t)",
        '\t\t(symbol\n\t\t\t"synth:U"',
        '\t\t\t(symbol "U_1_1"',
        '\t\t\t\t(pin input line\n\t\t\t\t\t(at 10 0 180)\n\t\t\t\t\t(number\n\t\t\t\t\t\t"1"\n\t\t\t\t\t)\n\t\t\t\t)',
        '\t\t\t\t(pin input line\n\t\t\t\t\t(at 10 -5 180)\n\t\t\t\t\t(number\n\t\t\t\t\t\t"2"\n\t\t\t\t\t)\n\t\t\t\t)',
        '\t\t\t\t(pin input line\n\t\t\t\t\t(at 10 -10 180)\n\t\t\t\t\t(number\n\t\t\t\t\t\t"3"\n\t\t\t\t\t)\n\t\t\t\t)',
        "\t\t\t)",
        "\t\t)",
        "\t)",
    ]
    for lib_id, ref, x, y, angle, mirror in placed:
        m = f"\n\t\t(mirror {mirror})" if mirror else ""
        out.append(
            f'\t(symbol\n\t\t(lib_id "{lib_id}")\n\t\t(at {x} {y} {angle}){m}\n'
            f'\t\t(property\n\t\t\t"Reference"\n\t\t\t"{ref}"\n\t\t\t(at 0 0 0)\n\t\t)\n\t)'
        )
    out.append(")")
    return "\n".join(out)


def netlist(*nets):
    """A kicad-cli netlist with nets given as (name, [(ref, pin), ...])."""
    out = ["(export", "\t(nets"]
    for code, (name, nodes) in enumerate(nets, 1):
        out.append(f'\t\t(net\n\t\t\t(code "{code}")\n\t\t\t(name "{name}")')
        for ref, pin in nodes:
            out.append(
                f'\t\t\t(node\n\t\t\t\t(ref "{ref}")\n\t\t\t\t(pin "{pin}")\n'
                '\t\t\t\t(pintype "passive")\n\t\t\t)'
            )
        out.append("\t\t)")
    out += ["\t)", ")"]
    return "\n".join(out)


R, D, U = "Device:R", "Device:D", "synth:U"


class PassiveDistance(unittest.TestCase):
    """How far a resistor or capacitor sits from the IC pin it is wired to."""

    def dist(self, sch_text, net_text):
        return sq.passive_pin_distances(sch_text, net_text)

    def test_the_distance_is_from_the_pad_to_the_ic_pin_on_its_net(self):
        # U1 pin 1 is at (10, 0). R1 at (20, 0): pad 1 at (20, -3.81).
        got = self.dist(
            parts_sch((U, "U1", 0, 0, 0, None), (R, "R1", 20, 0, 0, None)),
            netlist(("SIG", [("R1", 1), ("U1", 1)])),
        )
        self.assertAlmostEqual(got["R1"], (100 + 3.81**2) ** 0.5, places=3)

    def test_a_rotated_passive_moves_its_pads(self):
        # Turned 90 degrees counter-clockwise, pad 1 is at (16.19, 0).
        got = self.dist(
            parts_sch((U, "U1", 0, 0, 0, None), (R, "R1", 20, 0, 90, None)),
            netlist(("SIG", [("R1", 1), ("U1", 1)])),
        )
        self.assertAlmostEqual(got["R1"], 6.19, places=3)

    def test_a_mirrored_ic_moves_its_pins(self):
        # Mirrored about the y axis, U1 pin 1 is at (-10, 0).
        got = self.dist(
            parts_sch((U, "U1", 0, 0, 0, "y"), (R, "R1", -20, 0, 0, None)),
            netlist(("SIG", [("R1", 1), ("U1", 1)])),
        )
        self.assertAlmostEqual(got["R1"], (100 + 3.81**2) ** 0.5, places=3)

    def test_the_nearest_ic_pin_on_the_net_counts(self):
        # Both IC pins are on the net; pin 2 at (10, 5) is nearer pad 2 (20, 3.81).
        got = self.dist(
            parts_sch((U, "U1", 0, 0, 0, None), (R, "R1", 20, 0, 0, None)),
            netlist(("SIG", [("R1", 2), ("U1", 1), ("U1", 2)])),
        )
        self.assertAlmostEqual(got["R1"], (100 + 1.19**2) ** 0.5, places=3)

    def test_a_passive_with_no_ic_pin_on_its_nets_is_not_measured(self):
        got = self.dist(
            parts_sch((U, "U1", 0, 0, 0, None), (R, "R1", 20, 0, 0, None), (R, "R2", 40, 0, 0, None)),
            netlist(("A", [("R1", 1), ("R2", 1)]), ("B", [("U1", 1)])),
        )
        self.assertEqual(got, {})

    def test_the_ic_itself_is_not_a_passive(self):
        got = self.dist(
            parts_sch((U, "U1", 0, 0, 0, None), (R, "R1", 20, 0, 0, None)),
            netlist(("SIG", [("R1", 1), ("U1", 1)])),
        )
        self.assertEqual(set(got), {"R1"})

    def test_a_diode_or_led_counts_as_the_part_a_resistor_serves(self):
        # R1 limits the current of the LED D1 and shares a supply rail with the far-off U1.
        # R1 pad 1 is at (30, -3.81) and D1 pad 1 at (20, -3.81): 10 mm.
        got = self.dist(
            parts_sch(
                (U, "U1", 0, 0, 0, None),
                (D, "D1", 20, 0, 0, None),
                (R, "R1", 30, 0, 0, None),
            ),
            netlist(("LED", [("R1", 1), ("D1", 1)]), ("VIO", [("R1", 2), ("U1", 1)])),
        )
        self.assertAlmostEqual(got["R1"], 10.0, places=3)

    def test_a_resistor_named_like_a_connector_prefix_is_judged_by_its_symbol(self):
        # R_TNOW is a resistor (Device:R); CONN1 is not a capacitor although its
        # reference starts with C. Both are two-pin parts.
        got = self.dist(
            parts_sch(
                (U, "U1", 0, 0, 0, None),
                (D, "CONN1", 20, 0, 0, None),
                (R, "R_TNOW", 30, 0, 0, None),
            ),
            netlist(("SIG", [("R_TNOW", 1), ("CONN1", 1)]), ("X", [("U1", 1), ("CONN1", 2)])),
        )
        self.assertEqual(set(got), {"R_TNOW"})
        self.assertAlmostEqual(got["R_TNOW"], 10.0, places=3)

    def test_score_carries_the_mean_and_the_worst(self):
        got = sq.score(
            [], [], (100.0, 80.0), (), passive_distances={"R1": 4.0, "R2": 10.0}
        )
        self.assertEqual(got["passive_to_pin_mean_mm"], 7.0)
        self.assertEqual(got["passive_to_pin_max_mm"], 10.0)

    def test_no_measured_passives_scores_zero(self):
        got = sq.score([], [], (100.0, 80.0), ())
        self.assertEqual((got["passive_to_pin_mean_mm"], got["passive_to_pin_max_mm"]), (0.0, 0.0))


def labels_sch(*labels, angle=0, kind="label"):
    """A `.kicad_sch` with the IC `synth:U` placed as U1 at (50, 50) turned by `angle`.
    Its pins are 1 "A" at (-10, 0), 2 "B" at (-10, -5) (both facing left once
    unturned), 3 "C" at (10, 0) (facing right) and 4 "~" at (10, -5), and `labels`
    given as (text, x, y)."""
    out = [
        "(kicad_sch",
        "\t(lib_symbols",
        "\t\t(symbol\n\t\t\t\"synth:U\"",
        '\t\t\t(symbol "U_1_1"',
    ]
    for at, number, name in [
        ("-10 0 0", "1", "A"),
        ("-10 -5 0", "2", "B"),
        ("10 0 180", "3", "C"),
        ("10 -5 180", "4", "~"),
    ]:
        out.append(
            f"\t\t\t\t(pin input line\n\t\t\t\t\t(at {at})\n\t\t\t\t\t(length 3.81)\n"
            f'\t\t\t\t\t(name\n\t\t\t\t\t\t"{name}"\n\t\t\t\t\t)\n'
            f'\t\t\t\t\t(number\n\t\t\t\t\t\t"{number}"\n\t\t\t\t\t)\n\t\t\t\t)'
        )
    out += ["\t\t\t)", "\t\t)", "\t)"]
    out.append(
        f'\t(symbol\n\t\t(lib_id "synth:U")\n\t\t(at 50 50 {angle})\n'
        '\t\t(property\n\t\t\t"Reference"\n\t\t\t"U1"\n\t\t\t(at 0 0 0)\n\t\t)\n\t)'
    )
    for text, x, y in labels:
        out.append(f'\t({kind}\n\t\t"{text}"\n\t\t(at {x} {y} 0)\n\t\t(effects)\n\t)')
    out.append(")")
    return "\n".join(out)


def label_numbers(sch_text):
    got = sq.score([], [], (100.0, 80.0), (), label_pairs=sq.label_pin_pairs(sch_text))
    return got["labels_repeating_pin_name"], got["label_columns_misaligned"]


class LabelAlignment(unittest.TestCase):
    """Labels beside a symbol side: one column per side, and no label that only
    repeats the name of the pin beside it. Left pins are at x 40, right pins at x 60."""

    def test_labels_in_one_column_on_a_side_are_aligned(self):
        got = label_numbers(labels_sch(("SIG1", 34.92, 50), ("SIG2", 34.92, 55)))
        self.assertEqual(got, (0, 0))

    def test_labels_in_two_columns_on_a_side_are_misaligned(self):
        got = label_numbers(labels_sch(("SIG1", 34.92, 50), ("SIG2", 37.0, 55)))
        self.assertEqual(got, (0, 1))

    def test_a_column_per_side_is_aligned(self):
        got = label_numbers(labels_sch(("SIG1", 34.92, 50), ("SIG2", 65.08, 50)))
        self.assertEqual(got, (0, 0))

    def test_two_misaligned_sides_count_twice(self):
        got = label_numbers(
            labels_sch(
                ("S1", 34.92, 50), ("S2", 37.0, 55), ("S3", 65.08, 50), ("S4", 65.5, 55)
            )
        )
        self.assertEqual(got, (0, 2))

    def test_a_label_that_repeats_the_pin_name_is_counted(self):
        got = label_numbers(labels_sch(("A", 34.92, 50), ("SIG2", 34.92, 55)))
        self.assertEqual(got, (1, 0))

    def test_a_label_with_the_name_of_another_pin_is_not(self):
        got = label_numbers(labels_sch(("B", 34.92, 50)))
        self.assertEqual(got, (0, 0))

    def test_a_pin_without_a_name_repeats_nothing(self):
        got = label_numbers(labels_sch(("~", 65.08, 55)))
        self.assertEqual(got, (0, 0))

    def test_a_global_label_counts_like_a_label(self):
        got = label_numbers(
            labels_sch(("A", 34.92, 50), ("SIG2", 37.0, 55), kind="global_label")
        )
        self.assertEqual(got, (1, 1))

    def test_a_hierarchical_label_counts_like_a_label(self):
        got = label_numbers(
            labels_sch(("A", 34.92, 50), ("SIG2", 37.0, 55), kind="hierarchical_label")
        )
        self.assertEqual(got, (1, 1))

    def test_a_label_on_no_pin_row_is_ignored(self):
        got = label_numbers(labels_sch(("A", 34.92, 52), ("SIG2", 20.0, 50), ("SIG3", 34.92, 55)))
        self.assertEqual(got, (0, 0))

    def test_a_label_inside_the_symbol_is_not_beside_the_pin(self):
        got = label_numbers(labels_sch(("A", 45.0, 50), ("SIG2", 34.92, 55)))
        self.assertEqual(got, (0, 0))

    def test_a_turned_symbol_is_aligned_along_the_other_axis(self):
        # Turned 90 degrees, the left pins face down: pin 1 at (50, 60), pin 2 at (55, 60).
        aligned = labels_sch(("S1", 50, 65.08), ("S2", 55, 65.08), angle=90)
        stepped = labels_sch(("S1", 50, 65.08), ("S2", 55, 65.5), angle=90)
        self.assertEqual(label_numbers(aligned), (0, 0))
        self.assertEqual(label_numbers(stepped), (0, 1))

    def test_a_schematic_without_labels_scores_zero(self):
        self.assertEqual(label_numbers(labels_sch()), (0, 0))


if __name__ == "__main__":
    unittest.main()
