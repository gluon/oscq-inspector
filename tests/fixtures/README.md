# Fixtures

Inputs for the `oscq-core` tests. Provenance of every file is listed here. A fixture copied from a third party is kept byte for byte identical to its source, so a test cannot be made to pass by editing its reference.

## Real server captures

| File | Source |
| --- | --- |
| `helper-root.json` | GET `/` on Vidvox OSCQuery Helper serving `SampleDocument`, HTTP port 2345, captured on 2026-10-09 |
| `helper-hostinfo.json` | GET `/?HOST_INFO` on the same Helper, OSC target port 1240 |
| `max-root.json` | GET `/` on Max 9, which serves OSCQuery natively on HTTP port 30339, running `max/test.maxpat`; added in commit `1bb8919` |
| `max-hostinfo.json` | GET `/?HOST_INFO` on the same Max: the same bytes as `max-root.json` |
| `max/test.maxpat` | the Max 9.2 patcher served for these captures |

The Helper declares `ACCESS` 2, write only, on every node, by design: it forwards to an OSC target whose address space it cannot read (`OSCQueryHelper/OSCQueryHelperAppDelegate.m`, VVOSCQueryProtocol commit `cebb6f8`). Its `HOST_INFO` has no `OSC_IP`, `WS_IP` or `WS_PORT` and advertises no WebSocket extension, the nominal case.

Max serves three parameters, each attribute of a parameter a node of its own (`longname`, `min`, `max`, `raw`, `normalized`...), 36 nodes on five levels. Methods are `ACCESS` 1 or 3 with real `VALUE`s, and no `EXTENSIONS` is listed. Max ignores the `HOST_INFO` query and answers with the whole tree: no `OSC_PORT`, no WebSocket key. It listens on 127.0.0.1 only and does not advertise over Bonjour, so it is reached by manual entry.

Still missing: VDMX and ossia. They can only be captured on a machine running those servers. Procedure, for each server, with its HTTP host and port:

```
curl -s http://HOST:PORT/ -o tests/fixtures/SERVER_tree.json
curl -s "http://HOST:PORT/?HOST_INFO" -o tests/fixtures/SERVER_host_info.json
```

## What the captures contain

OSC paths, types, ranges, values, access flags, server names (`SampleDocument`) and port numbers (the Helper's OSC target port, 1240). No captured file carries an IP address, a machine name or a file path: the Helper's `HOST_INFO` gives no `OSC_IP` and no `WS_IP`, Max gives no `HOST_INFO` at all, and `max/test.maxpat` holds three parameter objects and no path. The only address among the fixtures is `127.0.0.1`, in the hand written `host_info/full.json`.

The addresses and host names in the tests' source code, such as `10.0.0.2`, `192.168.1.20`, `fe80::1` or `studio.local`, are examples written for the tests, taken from private, link local and `.local` ranges; they come from no capture.

## Third party

| File | Source | Licence |
| --- | --- | --- |
| `third_party/vvoscqueryprotocol_helper_sample_document.json` | `OSCQueryHelper/SampleDocument.json` in https://github.com/Vidvox/VVOSCQueryProtocol at commit `cebb6f89f40ad4174042dbb1625aef98132c6bef` | BSD 3-Clause, `third_party/VVOSCQueryProtocol-LICENSE` |

This is the input file OSCQuery Helper loads to describe an address space, not a document captured from a running server. It has no `FULL_PATH` anywhere, and it is not valid JSON: line 171 carries a trailing comma inside an array. The test asserts that it is rejected as not JSON.

## Hand written

Written for this project. Shapes follow the OSCQuery proposal, https://github.com/Vidvox/OSCQueryProposal, commit `e3d8e3b91020c617f47fd88de157a1fe053e0583`.

| File | Case |
| --- | --- |
| `documents/basic_tree.json` | containers, methods, `ACCESS` 0 to 3, `RANGE` with `MIN` `MAX` `VALS`, `TAGS`, `CRITICAL`, an unknown key kept verbatim |
| `documents/per_arg_nested.json` | per argument attributes as nested arrays, `TYPE` `i[ff]i` |
| `documents/per_arg_null.json` | per argument attributes with `null` entries |
| `documents/per_arg_scalar.json` | per argument attributes as a single scalar standing for every entry |
| `documents/method_without_type.json` | node without `CONTENTS` and without `TYPE`, node with both `CONTENTS` and `TYPE`, empty container |
| `documents/access_absent.json` | `ACCESS` absent, `ACCESS` 0, `ACCESS` out of range |
| `documents/value_length_mismatch.json` | `VALUE` shorter and longer than `TYPE`, `VALUE` without `TYPE` |
| `documents/invalid_osc_paths.json` | paths legal in JSON but not in OSC: space, `#`, `*`, non ASCII, control character |
| `documents/colors.json` | `r` without `EXTENDED_TYPE`, `color.rgb.*` and `color.rgba.*` spellings, `OVERLOADS` |
| `documents/no_payload_tags.json` | `T`, `I`, `N`, and an unknown type tag |
| `documents/missing_full_path.json` | `FULL_PATH` absent at every level, a `CONTENTS` entry that is not an object |
| `documents/not_json.txt` | body that is not JSON |
| `documents/not_an_object.json` | JSON that is not an object |
| `documents/not_an_address_space.json` | JSON object that is not an OSCQuery node |
| `host_info/full.json` | every `HOST_INFO` key |
| `host_info/no_osc_port.json` | `OSC_PORT` absent, inferred from the HTTP port |
| `host_info/tcp.json` | `OSC_TRANSPORT` `TCP`, sending blocked |
| `host_info/string_port.json` | ports as numeric strings, accepted as VVOSCQueryProtocol does |
| `host_info/invalid_port.json` | `OSC_PORT` out of range, server not controllable |
| `host_info/empty.json` | empty `HOST_INFO` |

Large and deep documents, 5000 nodes and nesting beyond the depth caps, are generated by the tests rather than committed.
