{
    "patcher": {
        "fileversion": 1,
        "appversion": {
            "major": 9,
            "minor": 2,
            "revision": 0,
            "architecture": "x64",
            "modernui": 1
        },
        "classnamespace": "box",
        "rect": [ 33.0, 100.0, 442.0, 983.0 ],
        "boxes": [
            {
                "box": {
                    "id": "obj-3",
                    "maxclass": "slider",
                    "min": -1000.0,
                    "numinlets": 1,
                    "numoutlets": 1,
                    "outlettype": [ "" ],
                    "parameter_enable": 1,
                    "patching_rect": [ 424.0, 509.0, 20.0, 140.0 ],
                    "saved_attribute_attributes": {
                        "valueof": {
                            "parameter_longname": "fader",
                            "parameter_mmax": 1000.0,
                            "parameter_mmin": -1000.0,
                            "parameter_modmode": 3,
                            "parameter_shortname": "fader",
                            "parameter_type": 0
                        }
                    },
                    "size": 2001.0,
                    "varname": "fader"
                }
            },
            {
                "box": {
                    "id": "obj-2",
                    "maxclass": "live.dial",
                    "numinlets": 1,
                    "numoutlets": 2,
                    "outlettype": [ "", "float" ],
                    "parameter_enable": 1,
                    "patching_rect": [ 457.0, 299.0, 41.0, 48.0 ],
                    "saved_attribute_attributes": {
                        "valueof": {
                            "parameter_longname": "freq",
                            "parameter_mmax": 1000.0,
                            "parameter_modmode": 3,
                            "parameter_osc_enabled": 1,
                            "parameter_osc_valuemode": 1,
                            "parameter_shortname": "freq",
                            "parameter_type": 0,
                            "parameter_unitstyle": 1
                        }
                    },
                    "varname": "freq"
                }
            },
            {
                "box": {
                    "id": "obj-1",
                    "maxclass": "live.dial",
                    "numinlets": 1,
                    "numoutlets": 2,
                    "outlettype": [ "", "float" ],
                    "parameter_enable": 1,
                    "patching_rect": [ 379.0, 295.0, 41.0, 48.0 ],
                    "saved_attribute_attributes": {
                        "valueof": {
                            "parameter_longname": "gain",
                            "parameter_modmode": 3,
                            "parameter_osc_enabled": 1,
                            "parameter_osc_valuemode": 1,
                            "parameter_shortname": "gain",
                            "parameter_type": 0,
                            "parameter_unitstyle": 0
                        }
                    },
                    "varname": "gain"
                }
            }
        ],
        "lines": [],
        "parameters": {
            "obj-1": [ "gain", "gain", 0 ],
            "obj-2": [ "freq", "freq", 0 ],
            "obj-3": [ "fader", "fader", 0 ],
            "parameterbanks": {
                "0": {
                    "index": 0,
                    "name": "",
                    "parameters": [ "-", "-", "-", "-", "-", "-", "-", "-" ],
                    "buttons": [ "-", "-", "-", "-", "-", "-", "-", "-" ]
                }
            },
            "inherited_shortname": 1
        },
        "autosave": 0
    }
}