# Fixture provenance

Official RRG/Iceman source commit: `fabf0f7b947628b9540a93f71c48c8ae4cd270c4` (24 October 2026).

- `credential-vectors.json`: 48 outputs from compiling the unchanged official C pack functions for H10301, H10306, H10304, H10302, C1k35s and C1k48s, including `add_HID_header`, in a separate work-only oracle. Each format has zero, maximum, mixed and boundary values. The implementation under test is Studio's independent Rust codec.
- `desfire-read.txt`: representative RRG printed data-table syntax, checked against the official command implementation. It is not a card capture.
- `simulated-ntag213.json`: synthetic 180-byte Studio dump. Page 4 contains `01020304`; all other bytes are zero. UID `04112233445566` is a mock-only identifier. This is test data, not a hardware backup.

Primary sources:

- [Wiegand formats](https://github.com/RfidResearchGroup/proxmark3/blob/fabf0f7b947628b9540a93f71c48c8ae4cd270c4/client/src/wiegand_formats.c)
- [HID transport headers](https://github.com/RfidResearchGroup/proxmark3/blob/fabf0f7b947628b9540a93f71c48c8ae4cd270c4/client/src/wiegand_formatutils.c)
- [DESFire commands](https://github.com/RfidResearchGroup/proxmark3/blob/fabf0f7b947628b9540a93f71c48c8ae4cd270c4/client/src/cmdhfmfdes.c)
- [RRG dump JSON](https://github.com/RfidResearchGroup/proxmark3/blob/fabf0f7b947628b9540a93f71c48c8ae4cd270c4/client/src/fileutils.c)

The 0.4.0 DESFire mock and archive fixtures are synthetic. Raw file/key-settings layouts, access-right nibble ordering, record offsets, value operations, ISO ID flags and the 1024-byte record-write limit were checked against the DESFire commands above. Simulation models expected adapter responses and interruptions; it does not implement or validate DESFire cryptography or RF behaviour. The real-client integration tests check command parsing/help, long-command transport and offline Wiegand encoding only. The Lua capture API was checked against `client/src/scripting.c`; the helper contains no command material or secrets.

`rrg-hardware-version.txt` is a synthetic version-output fixture using invented revision, source fingerprint, dates, port and hardware revision. It preserves RRG output syntax to test USB handshake detection and source-fingerprint mismatches. No company device capture is distributed.

## EM410x reader interpretations (0.5.1)

`tests/em410x.rs` checks documented examples and synthetic identifiers, not the user's fob ID:

- [Henrium EM4100/TK4100 number formats](https://www.henrium.com/guides/em4100-tk4100-cards-explained): example `1B02D4318F` gives facility/card 212/12687 for low-24-bit 8/16 and 724/12687 for low-32-bit 16/16; decimal 32-bit ID 47460751.
- [Suprema EM4100 card representation](https://support.supremainc.com/en/support/solutions/articles/24000040584--biostar-2-how-to-indicate-em4100-card-on-suprema-products): example customer byte 75 and ID 4340757, full 40-bit number 322126887957. This is a different interpretation from low-24-bit facility/card.
- [RRG EM410x display implementation](https://github.com/RfidResearchGroup/proxmark3/blob/fabf0f7b947628b9540a93f71c48c8ae4cd270c4/client/src/cmdlfem410x.c): local source inspected at the pinned revision, especially DEZ 3.5C and DEZ 5.5 numeric bit boundaries. No upstream implementation code is included in the new module.

These sources describe reader/display conventions, not a universal facility-code field on every EM410x fob. The synthetic `AB12FA4321` and `AB00FA4321` cases exercise a known-facility hint, distinguish high-byte handling and demonstrate ambiguous matching without copying the user's actual credential into the test suite.
