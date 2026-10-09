# OSCQuery protocol notes

Normative source for this project. Every item marked VERIFY in `PRD.md` is answered here with the primary source it was checked against. Where the written proposal is silent or contradicts itself, the reference implementation was read and the source used is named.

## Sources

| Id | Source | Revision read |
| --- | --- | --- |
| SPEC | https://github.com/Vidvox/OSCQueryProposal, `README.md` | commit `e3d8e3b91020c617f47fd88de157a1fe053e0583` (2018-08-31), last commit on the default branch at the time of reading |
| EXT | same repository, `extended_types.md` | same commit |
| REF | https://github.com/Vidvox/VVOSCQueryProtocol, reference Objective C / C++ implementation by Vidvox | commit `cebb6f89f40ad4174042dbb1625aef98132c6bef` (2024-05-07) |
| MDNS | `mdns-sd` crate 0.21.5, `src/service_daemon.rs` | as resolved in `Cargo.lock` |

Permalink prefixes used below:

- SPEC: `https://github.com/Vidvox/OSCQueryProposal/blob/e3d8e3b91020c617f47fd88de157a1fe053e0583/README.md`
- EXT: `https://github.com/Vidvox/OSCQueryProposal/blob/e3d8e3b91020c617f47fd88de157a1fe053e0583/extended_types.md`
- REF: `https://github.com/Vidvox/VVOSCQueryProtocol/blob/cebb6f89f40ad4174042dbb1625aef98132c6bef/`

The proposal describes itself as "the third revision of this proposal, which is in its later stages" (SPEC#L35). It is not a 1.0 release.

## 1. mDNS service type (PRD 8.1, VERIFY)

Confirmed: `_oscjson._tcp`. With `mdns-sd` the string to pass is `_oscjson._tcp.local.`.

- SPEC, section "Core Functionality", SPEC#L67: "advertise their presence (and look for other servers) on the local network using bonjour/zeroconf using the service type "_oscjson._tcp."."
- REF browses and registers the same string: `VVOSCQueryProtocol/VVOSCQueryServerDetectorDomain.m#L24` (`searchForServicesOfType:@"_oscjson._tcp."`), `VVOSCQueryProtocol/VVOSCQueryServer.mm#L367` (`type:@"_oscjson._tcp."`).
- MDNS: `browse()` rejects any type not ending in `._tcp.local.` or `._udp.local.` (`check_domain_suffix`, `service_daemon.rs` line 4735). So the `.local.` suffix in PRD 8.1 is required by the crate, not by the spec.

`_osc._udp`: not mentioned anywhere in SPEC, EXT or REF. Its status as a DNS-SD service type could not be checked here: the IANA service name registry is unreachable from this environment (proxy returned 403). To prove it: look up `osc` in https://www.iana.org/assignments/service-names-port-numbers/ and capture real advertisements on a LAN.

## 2. Attribute keys, exact spelling (PRD 7.1, VERIFY)

All keys are upper case ASCII with underscores. Confirmed spellings, from SPEC sections "Required Attributes" (SPEC#L75 to L89) and "Optional Attributes" (SPEC#L92 to L114), cross checked against REF constants `VVOSCQueryProtocol/VVOSCQueryConstants.m#L6-L33`:

| Key | SPEC class | JSON type | Notes |
| --- | --- | --- | --- |
| `FULL_PATH` | required | string | "Every OSC method is required to return a non-null value for this attribute." (SPEC#L79) |
| `CONTENTS` | required | object, child name to node | "If this string:value pair is missing, the corresponding node should be assumed to be an OSC method (rather than a container)." (SPEC#L80) |
| `TYPE` | required | string, OSC type tag string | "If an OSC method doesn't have a value and doesn't require a value to be sent to it (usually an indicator that this node is purely a container), this attribute may be omitted" (SPEC#L81) |
| `HOST_INFO` | required | object | Query only, never in a node document, see section 4 |
| `ACCESS` | optional | integer 0..3 | see section 3 |
| `VALUE` | optional | array, one item per type tag | see section 6 |
| `RANGE` | optional | array of objects with `MIN`, `MAX`, `VALS` | `VALS` is "an array listing the only acceptable values" (SPEC#L106) |
| `DESCRIPTION` | optional | string | REF classes it as required (`kVVOSCQ_ReqAttr_Desc`) |
| `TAGS` | optional | array of strings | |
| `EXTENDED_TYPE` | optional | array of strings, one per type tag | vocabulary in EXT |
| `UNIT` | optional | array of strings, one per type tag | SPEC#L206 once writes `UNITS`; the attribute definition (SPEC#L110) and REF use `UNIT` |
| `CRITICAL` | optional | boolean | |
| `CLIPMODE` | optional | array of strings, one per type tag | values exactly `none`, `low`, `high`, `both` (SPEC#L112, REF `VVOSCQueryConstants.m#L39-L42`) |
| `OVERLOADS` | optional | array of node objects without `CONTENTS` | |
| `HTML` | optional | query only | client asks for an HTML resource instead of JSON (SPEC#L114) |

Sub keys: `RANGE` items use `MIN`, `MAX`, `VALS`. `HOST_INFO` keys are in section 4. WebSocket keys are `COMMAND`, `DATA`, and for renames `OLD`, `NEW` (section 7).

## 3. ACCESS semantics (PRD 7.1, VERIFY)

SPEC, "Optional Attributes", SPEC#L96: "an integer that represents a binary mask. Returns 0 if there is no value associated with this OSC destination, 1 if the value may only be retrieved, 2 if the value may only be set, or 3 if the value may be both retrieved and set."

So bit 0 is readable, bit 1 is writable. 0 means "no value", not "no access".

Default when absent, SPEC#L99: "If the ACCESS attribute is not supported, or it is supported but a value is not returned for it, then the assumption should be that the value is always writable, and also readable if the "VALUE" attribute is supported."

"Supported" is defined by `HOST_INFO.EXTENSIONS` (SPEC#L84, SPEC#L94). REF servers always emit `ACCESS` (`VVOSCQueryServer/OSCNodeAdditions.m#L170-L172`).

## 4. HOST_INFO (PRD 7.2, VERIFY)

Request: any path with the `HOST_INFO` query parameter. SPEC#L82: "If this attribute is queried, the JSON object that is returned will *only* consist of this object- the path in the URL is functionally irrelevant." REF client requests `http://host:port?HOST_INFO` (`VVOSCQueryProtocol/VVOSCQueryRemoteServer.mm#L391`). The response is the HOST_INFO object itself, not wrapped under a `HOST_INFO` key (SPEC example at SPEC#L358 to L376).

Keys, SPEC#L83 to L89, REF `VVOSCQueryConstants.m#L10-L17`. All optional ("zero or more of the following keys"):

| Key | Type | Default when absent, per SPEC |
| --- | --- | --- |
| `NAME` | string | none, may be omitted |
| `EXTENSIONS` | object, key to boolean | "if an optional attribute is not listed, the client should assume that it isn't supported" |
| `OSC_IP` | string | same network address as the OSCQuery HTTP server |
| `OSC_PORT` | integer | same as the OSCQuery HTTP server, see below |
| `OSC_TRANSPORT` | string, `"TCP"` or `"UDP"` | `UDP` |
| `WS_IP` | string | same address as the HTTP server |
| `WS_PORT` | integer | "the same port as the OSCQuery server which is serving this request" |

`OSC_PORT` default: the key definition (SPEC#L86) says "the same network address as the OSCQuery server", and the worked example (SPEC#L353) says "The HOST_INFO object doesn't have any entries for "OSC_IP" or "OSC_PORT", so the OSC server is running on the same IP/port as the OSCQuery server." REF client implements the example: absent `OSC_PORT` falls back to the HTTP port (`VVOSCQueryRemoteServer.mm#L260-L270`), and accepts the port as a JSON number or as a numeric string.

`EXTENSIONS` keys: the names of optional attributes (`ACCESS`, `VALUE`, `RANGE`, `DESCRIPTION`, `TAGS`, `EXTENDED_TYPE`, `UNIT`, `CRITICAL`, `CLIPMODE`, `OVERLOADS`, `HTML`) and of WebSocket commands (`LISTEN`, `IGNORE`, `PATH_CHANGED`, `PATH_RENAMED`, `PATH_REMOVED`, `PATH_ADDED`). SPEC example SPEC#L363 to L375 shows `LISTEN` and `PATH_CHANGED`. SPEC#L154: "It is not necessary to list the "IGNORE" attribute in the server's EXTENSIONS object if "LISTEN" is already listed". REF OSCQuery Helper advertises `TAGS`, `ACCESS`, `VALUE`, `RANGE`, `HTML`, `PATH_CHANGED`, with `LISTEN` commented out (`OSCQueryHelper/OSCQueryHelperAppDelegate.m#L531-L547`), and always sets `OSC_TRANSPORT` to `UDP` when missing (L528-L529).

There is no key that advertises WebSocket support as such. SPEC#L141: "It is assumed that the websocket server is running on the same IP/port as the HTTP server, but this can be verified by retrieving the server's HOST_INFO object and checking its contents for "WS_IP" and "WS_PORT"." REF client always attempts a WebSocket connection to `ws://WS_IP:WS_PORT`, falling back to the HTTP address and port, with no EXTENSIONS check (`VVOSCQueryRemoteServer.mm#L285-L308`).

## 5. Attribute query syntax (PRD 7.3, VERIFY)

SPEC#L64: "The URL http://localhost:1234/foo/bar?VALUE refers to the "VALUE" attribute of the method "/foo/bar"". The attribute is the bare query key with no value: `/path?ATTR`. The fragment is ignored (SPEC#L65).

Reply shape: an object containing only that key, for example `{"VALUE": [0.5]}` (SPEC#L314 to L322). A query for an attribute the node does not have returns `{}` (SPEC#L340 to L346, `/baz?TYPE`). A `VALUE` query on a method with no value yet returns an empty object (SPEC#L102).

Several attributes in one query: not defined by SPEC. REF server parses every query item and returns each requested key the node has (`VVOSCQueryProtocol/VVOSCQueryServer.mm#L41-L62`, `VVOSCQueryServer/OSCNodeAdditions.m#L77-L97`, called by OSCQuery Helper at `OSCQueryHelper/OSCQueryHelperAppDelegate.m#L559-L572`), so `?VALUE&RANGE` works against REF servers. Source used: REF. Do not rely on it for other servers.

Recursion: a plain GET returns the node and all its descendants ("the default is to fetch the full "tree" of the specified node and all its sub-nodes", SPEC#L284). REF makes attribute queries non recursive (`VVOSCQueryServer.mm#L102-L107`).

REF client percent encodes the OSC path before building the URL (`VVOSCQueryRemoteServer.mm#L461`, `sanitizedOSCAddress`).

## 6. VALUE, RANGE and per argument arrays (PRD 7.1, VERIFY)

SPEC#L118 to L121, "Other notes on attributes":

- `VALUE`, `RANGE`, `EXTENDED_TYPE`, `UNIT`, `CLIPMODE` hold one item per OSC type in `TYPE`.
- Type tag strings may contain `[` and `]`. The JSON arrays then nest to match: `TYPE` `i[ff]i` gives `VALUE` `[100, [1.0, 1.0], 100]` (SPEC#L427 to L452).
- A missing per argument entry is JSON `null`.
- "If all of the elements of one of these optional attribute share the same value, it's acceptable to only list a single value instead of an array". So a scalar in place of the array is legal. REF has a server switch to emit scalars (`OSCNodeAdditions.m#L9`, off by default).

JSON encoding per OSC type, SPEC#L125 to L133: `i f h d t` number; `s S c` string; `r` string `#RRGGBBAA`; `[ ]` array; `b m N I` null; `T F` boolean, and T and F are interchangeable for sending.

## 7. WebSocket commands, both directions (PRD 7.4, VERIFY)

Text frames carry JSON objects with `COMMAND` (string) and `DATA` (SPEC#L142 to L144). Names checked against REF `VVOSCQueryConstants.m#L49-L57`.

Client to server (SPEC#L147 to L154):

| COMMAND | DATA |
| --- | --- |
| `LISTEN` | string, OSC path. Not recursive: "if a client asks to "LISTEN" to /foo, it will only receive messages sent to /foo- it will not receive messages sent to /foo/bar." (SPEC#L151) |
| `IGNORE` | string, OSC path |

Server to client (SPEC#L156 to L167):

| COMMAND | DATA | Meaning |
| --- | --- | --- |
| `PATH_CHANGED` | string, OSC path | Structure changed: "the client should request its structure as soon as possible", the path "(and any methods it contained) must all be "reloaded"". Sent after adds, removes and renames, and may coalesce several changes into the highest common path (SPEC#L158 to L160) |
| `PATH_RENAMED` | object `{"OLD": string, "NEW": string}` | Listens follow the rename on both sides (SPEC#L161 to L165) |
| `PATH_REMOVED` | string, OSC path | |
| `PATH_ADDED` | string, OSC path | |

`PATH_CHANGED` is not a value notification. Values travel as binary OSC frames after `LISTEN`. REF browser reacts to all four server commands with a structural reload (`VVOSCQueryBrowser/ServerUIController.m#L210-L225`). REF client parses `PATH_RENAMED` only when `DATA` is an object with string `OLD` and `NEW` (`VVOSCQueryProtocol/WSPPClient.cpp#L73-L98`).

Reconnection, SPEC#L152: after a broken connection "it becomes the client's responsibility to re-establish the websocket connection, and then explicitly request to "LISTEN" to any OSC methods it wants to receive data from again."

## 8. OSC over the WebSocket (PRD 7.4 and 7.5, VERIFY)

Binary frames carry raw OSC packets, "identical to the format of OSC packets sent over the network" (SPEC#L145, SPEC#L150). No framing, no length prefix, one packet per frame.

Direction: SPEC#L145 says "This allows values to be streamed between the server and its clients" without stating that the server must accept OSC from the client. REF server accepts binary frames from clients and hands them to its OSC handler (`VVOSCQueryProtocol/WSPPServer.cpp#L308-L313`, `VVOSCQueryServer.mm#L195-L202`), and REF client can send binary frames (`WSPPClient.cpp#L288-L302`). Source used: REF. REF browser itself sends values over UDP, and refuses to send at all when `OSC_TRANSPORT` is `TCP` (`VVOSCQueryBrowser/ServerUIController.m#L128-L148`).

## 9. HTTP status codes (PRD 11, VERIFY)

SPEC#L69 to L73, "Core Functionality":

| Code | Meaning per SPEC |
| --- | --- |
| 204 | "no content- server received request, but the request is inappropriate in some way- querying VALUE when ACCESS returned 0, for example" |
| 400 | "bad request/syntax error- corrupt message/packet, querying an unrecognized attribute or a recognized attribute that isn't supported by the server, etc" |
| 404 | "no container or method was found at the supplied OSC address" |
| 408 | "request time-out- this error should be generated by the client" |

Contradiction inside SPEC: SPEC#L64 says a query "for an attribute that the server doesn't support, it should return an error of type 204", while SPEC#L71 assigns that case to 400. REF server never emits 204: it returns 404 for unknown paths and unparsable URLs, and for an unsupported attribute it returns `{}` with 200 (`VVOSCQueryProtocol/VVOSCQueryServer.mm#L45`, L130, L148, `VVOSCQueryServer/OSCNodeAdditions.m#L77-L106`, `OSCQueryHelper/OSCQueryHelperAppDelegate.m#L569-L571`). Source used: REF, which means a client must accept 200 with `{}`, 204, and 400 as "attribute not available".

## 10. Discrepancies between PRD.md and the sources

Listed for the human. PRD.md was not edited.

1. PRD 4, reference implementation URL: PRD points to `https://github.com/Vidvox/VVOpenSource`. SPEC#L12 points to `https://github.com/Vidvox/VVOSCQueryProtocol`, which exists and was used. `VVOpenSource` could not be cloned here (git asked for credentials, which is how GitHub answers for a missing or private repository); whether it exists is not proven.
2. PRD 7.1, `ACCESS` 0 is described as "no access". SPEC: 0 means "there is no value associated with this OSC destination" (SPEC#L96).
3. PRD 7.1, missing `ACCESS` "must be treated as read and write". SPEC: always writable, readable only if `VALUE` is supported per `EXTENSIONS` (SPEC#L99).
4. PRD 7.1, "A node whose `TYPE` is absent is a container, even if it has no `CONTENTS`." SPEC: a node without `CONTENTS` is a method (SPEC#L80), and `TYPE` may be omitted on a method that takes no value (SPEC#L81). The PRD rule inverts the SPEC rule for a method without `TYPE`.
5. PRD 7.1, `VALUE` "one per type tag character" and `RANGE` "one entry per argument": SPEC allows nested arrays for `[` `]` in `TYPE`, `null` placeholders, and a single scalar in place of the array when all entries are equal (SPEC#L118 to L121). The same applies to `EXTENDED_TYPE`, `UNIT`, `CLIPMODE`. The PRD type parser requirement omits `[` and `]`.
6. PRD 7.1 lists no `HTML` attribute and no `HOST_INFO` as an attribute; both are defined by SPEC (SPEC#L82, SPEC#L114). Not a defect for a client, noted for the deserialiser's known key list.
7. PRD 7.2, "A GET on the root with the `HOST_INFO` query parameter": SPEC says the path is irrelevant (SPEC#L82). Compatible, not wrong.
8. PRD 7.2, "If `HOST_INFO` fails or omits the OSC port, the server is browsable but not controllable ... do not guess a port." SPEC defines the default when `OSC_PORT` is absent: same IP and port as the OSCQuery server (SPEC#L86, SPEC#L353), and REF implements it (`VVOSCQueryRemoteServer.mm#L260-L270`). The PRD rule contradicts SPEC for the "omits" case. The "fails" case (no HOST_INFO at all) is not covered by SPEC. Decision needed from the human.
9. PRD 7.2 lists the transport as "UDP or TCP" but nothing in PRD 7.5 says what to do with `OSC_TRANSPORT` = `TCP`. SPEC does not define OSC over TCP framing. REF browser refuses to send in that case. Underspecified.
10. PRD 7.4, "When `HOST_INFO` advertises WebSocket support": SPEC has no such advertisement. WebSocket is assumed on the HTTP address and port unless `WS_IP` / `WS_PORT` say otherwise (SPEC#L141, SPEC#L88 to L89). `EXTENSIONS.LISTEN` and `EXTENSIONS.PATH_CHANGED` advertise features over the socket, not the socket itself. REF client always tries to connect. The trigger for opening the socket needs a decision.
11. PRD 7.4, "On `PATH_CHANGED`, re-fetch that path's `VALUE`": wrong. `PATH_CHANGED` signals a structural change and the path's subtree must be reloaded (SPEC#L158 to L160). Value changes arrive as binary OSC frames after `LISTEN`.
12. PRD 7.4, "On `PATH_ADDED` or `PATH_REMOVED`, re-fetch the parent container's subtree": compatible with SPEC, but SPEC says servers send `PATH_CHANGED` for adds, removes and renames too (SPEC#L159), possibly coalesced to a higher path (SPEC#L160). PRD gives no rule for `PATH_CHANGED` reload scope.
13. PRD 7.4 and 6.3 do not state the `PATH_RENAMED` payload shape. SPEC: `DATA` is `{"OLD": ..., "NEW": ...}` (SPEC#L161 to L163). `Event::PathRenamed { old, new }` matches.
14. PRD 7.4, LISTEN discipline: SPEC `LISTEN` is not recursive (SPEC#L151). A visible container row needs no LISTEN; each visible method needs its own. PRD does not say this.
15. PRD 7.5, "prefer the WebSocket for sends": SPEC does not require servers to accept OSC from clients over the socket (SPEC#L145). REF server does. Interop with ossia and others is unproven.
16. PRD 7.5, "Apply `CLIPMODE` and `RANGE` when clamping is requested by the node": SPEC defines `CLIPMODE` as a hint about how the server itself clips (SPEC#L112), not as a request for the client to clamp. The semantics need a decision.
17. PRD 9.3, `I` maps to a push button: matches SPEC (SPEC#L192, `N,I` momentary button). `N` is not listed in PRD 9.3 and falls to "anything else".
18. PRD 9.3, colour picker for "`r` or `ffff` named as a colour by `EXTENDED_TYPE`": SPEC treats `r` as a colour on its own (SPEC#L129, SPEC#L190). For `ffff`, the colour names are inconsistent inside the proposal: EXT lists `color.rgb.[rgba]` (EXT line 44) while SPEC's example uses `color.rgba.r` (SPEC#L477 to L480). Both spellings must be accepted.
19. PRD 11, error cases: "returned a non 200 status" as an error. SPEC defines 204 as a legitimate "inappropriate request" answer, and REF answers unsupported attributes with 200 and `{}`. A single value refresh must treat 204, 400 and `{}` as "value not available", not as a transport error. Also SPEC#L64 and SPEC#L71 contradict each other on 204 vs 400 (section 9).
20. PRD 8.1, `_osc._udp.local.`: not part of the OSCQuery proposal. Its use is a product choice, not a spec requirement. Registration status not proven here (section 1).
21. PRD header, crate name `void-oscq` free on crates.io: confirmed on 2026-10-09, `GET https://crates.io/api/v1/crates/void-oscq` returned 404, control request for `rosc` returned 200. All three crates are `publish = false` until a publication decision.
