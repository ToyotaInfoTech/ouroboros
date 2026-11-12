# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Tests for ouroboros.util module."""

import os
import tempfile

from ouroboros.util import configure_global_logging


class TestConfigureGlobalLogging:
    """Tests for configure_global_logging function."""

    def setup_method(self):
        """Reset global state before each test."""
        import ouroboros.util as util_mod

        util_mod.global_log_config = None

    def test_none_config_does_nothing(self):
        """Passing None should not set global_log_config."""
        configure_global_logging(None)

        import ouroboros.util as util_mod

        assert util_mod.global_log_config is None

    def test_valid_config_sets_global(self):
        """Providing a valid config dict sets global_log_config and configures logging."""
        config = {
            "version": 1,
            "handlers": {
                "console": {
                    "class": "logging.StreamHandler",
                    "level": "DEBUG",
                }
            },
            "root": {
                "level": "DEBUG",
                "handlers": ["console"],
            },
        }
        configure_global_logging(config)

        import ouroboros.util as util_mod

        assert util_mod.global_log_config is config

    def test_creates_log_directory(self):
        """Log directories specified in handler filenames should be created."""
        with tempfile.TemporaryDirectory() as tmpdir:
            log_file = os.path.join(tmpdir, "subdir", "test.log")
            config = {
                "version": 1,
                "handlers": {
                    "file": {
                        "class": "logging.FileHandler",
                        "filename": log_file,
                        "level": "DEBUG",
                    }
                },
                "root": {
                    "level": "DEBUG",
                    "handlers": ["file"],
                },
            }
            configure_global_logging(config)
            assert os.path.isdir(os.path.join(tmpdir, "subdir"))

    def test_second_call_is_noop(self):
        """Calling configure_global_logging twice should keep the first config."""
        first_config = {
            "version": 1,
            "handlers": {
                "console": {
                    "class": "logging.StreamHandler",
                    "level": "DEBUG",
                }
            },
            "root": {
                "level": "DEBUG",
                "handlers": ["console"],
            },
        }
        second_config = {
            "version": 1,
            "handlers": {},
            "root": {"level": "WARNING", "handlers": []},
        }
        configure_global_logging(first_config)
        configure_global_logging(second_config)

        import ouroboros.util as util_mod

        assert util_mod.global_log_config is first_config
