import json

from Util import Logging

MASTER_CONFIG = dict()
MASTER_LOADED = False


def load_master():
    global MASTER_CONFIG, MASTER_LOADED
    try:
        with open("config/master.json", "r") as file:
            MASTER_CONFIG = json.load(file)
            MASTER_LOADED = True
    except Exception as e:
        Logging.error(f"Failed to load master config: {e}")
        raise e


def get_master_var(key):
    global MASTER_LOADED, MASTER_CONFIG
    if not MASTER_LOADED:
        load_master()
    if key not in MASTER_CONFIG.keys():
        raise KeyError(f"Key {key} not found in master config")
    return MASTER_CONFIG[key]


def is_dev_env():
    return get_master_var("ENV") == "dev"
