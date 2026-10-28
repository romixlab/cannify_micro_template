"""Checks that need no board: the module loads and carries its facts. Tests with a board live with the firmware."""

import cannify_micro


def test_module_facts():
    assert cannify_micro.USB_ID == (0xC0DE, 0xCAFE)
    assert cannify_micro.__version__
    assert callable(cannify_micro.Device)
