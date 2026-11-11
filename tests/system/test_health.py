"""etc/deck-health: what counts as a world update (portage.py), which installed package is the
running kernel, the host configuration and the NVMe controllers (check.py); etc/deck-vitals: the
power supplies by type and the memory threshold."""
import pathlib
import tempfile
import unittest

from _load import DECK, script

portage = script(DECK / "etc/deck-health/portage.py", "deck_health_portage")
check = script(DECK / "etc/deck-health/check.py", "deck_health_check")
vitals = script(DECK / "etc/deck-vitals/vitals.py", "deck_vitals")


class WorldUpdate(unittest.TestCase):
    def test_world_updates(self):
        for args in ("--update --deep --newuse @world", "-avuDN @world", "-uDN world", "--ask -u @world",
                     "--usepkg-exclude dev-util/foo -avuDN @world"):
            self.assertTrue(portage.is_world_update(args), args)

    def test_others(self):
        for args in ("-avuDN @system", "-pvuDN @world", "--pretend --update @world", "-av @world",
                     "--depclean", "-u app-misc/foo", "--sync", ""):
            self.assertFalse(portage.is_world_update(args), args)


class Kernel(unittest.TestCase):
    NAMES = ["gentoo-kernel-bin-6.18.5", "gentoo-kernel-bin-6.18.54", "gentoo-kernel-6.18.5-r2",
             "gentoo-sources-6.18.5", "gentoo-kernel-bin-6.18.50-r1", "linux-firmware-20261001"]

    def test_exact_version_only(self):
        self.assertEqual(check.kernel_packages(self.NAMES, "6.18.5"),
                         [("gentoo-kernel", "gentoo-kernel-6.18.5-r2"), ("gentoo-kernel-bin", "gentoo-kernel-bin-6.18.5")])
        self.assertEqual(check.kernel_packages(self.NAMES, "6.18.54"), [("gentoo-kernel-bin", "gentoo-kernel-bin-6.18.54")])
        self.assertEqual(check.kernel_packages(self.NAMES, "6.18.50"), [("gentoo-kernel-bin", "gentoo-kernel-bin-6.18.50-r1")])

    def test_no_match(self):
        self.assertEqual(check.kernel_packages(self.NAMES, "6.18"), [])
        self.assertEqual(check.kernel_packages(self.NAMES, "6.18.540"), [])
        self.assertEqual(check.kernel_packages(["gentoo-kernel-bin-6x18y5"], "6.18.5"), [])


class HostConf(unittest.TestCase):
    def test_absent_is_empty(self):
        self.assertEqual(check.host_conf(pathlib.Path("/nonexistent/host.conf")), {})

    def test_values(self):
        with tempfile.TemporaryDirectory() as d:
            f = pathlib.Path(d, "host.conf")
            f.write_text('# a comment\nBTRFS_POOL=/mnt/pool\nNVME="/dev/nvme0 /dev/nvme1"\nESP=\n\nnot a line\n')
            self.assertEqual(check.host_conf(f), {"BTRFS_POOL": "/mnt/pool", "NVME": "/dev/nvme0 /dev/nvme1", "ESP": ""})

    def test_no_nvme(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(check.nvme_controllers(sys=d), [])
            pathlib.Path(d, "class/nvme/nvme1").mkdir(parents=True)
            self.assertEqual(check.nvme_controllers(sys=d), ["/dev/nvme1"])


class Vitals(unittest.TestCase):
    def test_supplies_by_type(self):
        with tempfile.TemporaryDirectory() as d:
            for name, kind, scope in (("ADP1", "Mains", None), ("hid-mouse", "Battery", "Device"),
                                      ("CMB0", "Battery", "System"), ("usb", "USB", None)):
                pathlib.Path(d, name).mkdir()
                pathlib.Path(d, name, "type").write_text(kind + "\n")
                if scope:
                    pathlib.Path(d, name, "scope").write_text(scope + "\n")
            self.assertEqual(vitals.supply("Mains", d), d + "/ADP1")
            self.assertEqual(vitals.supply("Battery", d), d + "/CMB0")

    def test_machine_without_them(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertIsNone(vitals.supply("Mains", d))
            self.assertIsNone(vitals.supply("Battery", d))

    def test_memory_threshold(self):
        self.assertAlmostEqual(vitals.short_of_memory(31), 12)
        self.assertAlmostEqual(vitals.short_of_memory(62), 24)


if __name__ == "__main__":
    unittest.main()
