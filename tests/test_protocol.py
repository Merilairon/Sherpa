"""Golden-byte tests: each layout here was read off SDKDLL.dll (Base Camp 1.9.10)."""

import unittest

from basecamp_linux import hidraw, keys, macros, protocol as p


class PacketTests(unittest.TestCase):
    def test_every_packet_is_64_bytes(self):
        pkts = [p.ap_enable(True), p.save_flash(6), p.switch_to_custom(80),
                p.change_effect(p.Effect(1, colors=[(1, 2, 3)]))]
        pkts += p.custom_static([(0, 0, 0)] * p.LED_COUNT)
        pkts += p.customize_table([0] * p.LED_COUNT)
        pkts += p.macro(0x24, [p.MacroEvent(36, p.ACT_PRESS)] * 20)
        self.assertTrue(all(len(x) == 64 for x in pkts))

    def test_switch_to_custom_is_ff_filled(self):
        pkt = p.switch_to_custom(0x50)
        self.assertEqual(pkt[:6], bytes([0x14, 0x2C, 0x0A, 0x00, 0xFF, 0x50]))
        self.assertEqual(set(pkt[6:]), {0xFF})

    def test_custom_static_chunks(self):
        colors = [(i, i, i) for i in range(p.LED_COUNT)]
        pkts = p.custom_static(colors)
        self.assertEqual(len(pkts), 7)  # 378 bytes / 57
        for i, pkt in enumerate(pkts):
            self.assertEqual(pkt[:7], bytes([0x14, 0x2C, 0x00, 0x01, i, 0x4B, 0x00]))
        self.assertEqual(pkts[0][7:10], bytes([0, 0, 0]))
        self.assertEqual(pkts[1][7:10], bytes([19, 19, 19]))  # LED 19 starts chunk 1
        self.assertEqual(pkts[6][7 + 36 :], bytes(21))  # last chunk: 36 data bytes

    def test_customize_table_chunks(self):
        pkts = p.customize_table(list(range(p.LED_COUNT)))
        self.assertEqual([x[:4] for x in pkts],
                         [bytes([0x14, 0xA0, i, 0x01]) for i in range(3)])
        self.assertEqual(pkts[2][4:10], bytes(range(120, 126)))

    def test_change_effect_static(self):
        pkt = p.change_effect(p.Effect(0, brightness=100, speed=10, colors=[(255, 0, 0)]))
        self.assertEqual(pkt[:9], bytes([0x14, 0x2C, 0, 0, 0xFF, 100, 0, 0xFF, 0xFF]))
        self.assertEqual(pkt[9:12], bytes([255, 0, 0]))

    def test_change_effect_speed_remap(self):
        pkt = p.change_effect(p.Effect(1, speed=50))  # breathing, middle speed
        self.assertEqual(pkt[4], 3)

    def test_change_effect_rejects_block_effects(self):
        with self.assertRaises(ValueError):
            p.change_effect(p.Effect(p.EFFECTS["colorwave"]))

    def test_save_flash(self):
        self.assertEqual(p.save_flash(6)[:5], bytes([0x13, 0x55, 0, 0, 6]))

    def test_macro_framing(self):
        evs = [p.MacroEvent(36, p.ACT_PRESS, delay_ms=0x1234)] * 16  # +END = 17 events
        pkts = p.macro(0x7B, evs, playback=1, times=1)
        self.assertEqual(pkts[0][:5], bytes([0x14, 0x18, 1, 0, 1]))
        self.assertEqual(pkts[1][:4], bytes([0x15, 0x7B, 15, 0]))
        self.assertEqual(pkts[1][4:8], bytes([36, 0x02, 0x34, 0x12]))
        self.assertEqual(pkts[2][:4], bytes([0x15, 0x7B, 2, 1]))
        self.assertEqual(pkts[2][8:12], bytes([0xFF, 0xFF, 0, 0]))  # terminator

    def test_reply_matching(self):
        req = p.save_flash(1)
        self.assertTrue(p.is_reply_to(req, req[:4] + bytes(60)))
        self.assertTrue(p.is_nak(bytes([0xFF, 0xAA]) + bytes(62)))


class MacroFileTests(unittest.TestCase):
    def test_basecamp_inputsjson_shape(self):
        ev = macros.event_from_json({"type": "keydown", "rawcode": 72, "delay": 5})
        self.assertEqual(ev, p.MacroEvent(keys.by_name("H").dll_key, p.ACT_PRESS, p.PAGE_KEY, 5))
        wheel = macros.event_from_json({"type": "wheel", "rotation": -1, "delay": 0})
        self.assertEqual((wheel.key, wheel.page, wheel.action), (p.WHEEL_UP, p.PAGE_MOUSE, p.ACT_WHEEL))


class DescriptorTests(unittest.TestCase):
    def test_vendor_collection_with_report_id(self):
        desc = bytes([
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, 0x01, 0x95, 0x08, 0x81, 0x02, 0xC0,
            0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x07, 0x95, 0x40, 0x91, 0x02, 0xC0,
        ])
        self.assertEqual(hidraw.vendor_report_id(desc), 7)

    def test_vendor_collection_unnumbered(self):
        desc = bytes([0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x95, 0x40, 0x81, 0x02, 0xC0])
        self.assertEqual(hidraw.vendor_report_id(desc), 0)

    def test_no_vendor_collection(self):
        desc = bytes([0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x81, 0x02, 0xC0])
        self.assertIsNone(hidraw.vendor_report_id(desc))


if __name__ == "__main__":
    unittest.main()
