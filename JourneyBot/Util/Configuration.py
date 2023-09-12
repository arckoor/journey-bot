import json

from Util import Logging

MASTER_CONFIG = dict()
MASTER_LOADED = False


def save_master_var():
    global MASTER_CONFIG
    with open("config/master.json", "w") as file:
        json.dump(MASTER_CONFIG, file, indent=4, skipkeys=True, sort_keys=True)


def load_master():
    global MASTER_CONFIG, MASTER_LOADED
    try:
        with open("config/master.json", "r") as file:
            MASTER_CONFIG = json.load(file)
            MASTER_LOADED = True
    except Exception as e:
        Logging.error(f"Failed to load master config: {e}")
        raise e


def get_master_var(key, default=None):
    global MASTER_LOADED, MASTER_CONFIG
    if not MASTER_LOADED:
        load_master()
    if key not in MASTER_CONFIG.keys():
        MASTER_CONFIG[key] = default
        save_master_var()
    return MASTER_CONFIG[key]


def is_dev_env():
    return get_master_var("ENV") == "dev"
