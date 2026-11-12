# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

import logging
import os
from logging import config as logging_config
from pprint import pformat

_logger = logging.getLogger(__name__)

global_log_config = None


def configure_global_logging(log_config: dict | None = None):
    """
    Set up global logging configuration.

    Args:
        log_config (dict | None): Logging configuration dictionary.
    """
    # logging.basicConfig(encoding="utf-8", level=logging.INFO)

    global global_log_config
    if log_config is not None and global_log_config is None:
        global_log_config = log_config
        for handler in log_config["handlers"].values():
            if "filename" in handler:
                log_dir = os.path.dirname(handler["filename"])
                os.makedirs(log_dir, exist_ok=True)

        # print(f"Configuring global logging with: {pformat(log_config)}")
        logging_config.dictConfig(log_config)
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Configured global logging with: {pformat(log_config)}")
