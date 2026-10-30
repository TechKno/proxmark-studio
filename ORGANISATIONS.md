# Organisation registers

Each organisation register is a private JSON file with its own users, fobs, history and reserved numbers. The active file path is stored in private app settings. Organisation mode was previously labelled NetAccess; the persisted mode value and old file names are recognised for compatibility.

## Configure a site

Choose **New organisation** and set the organisation/site names, EM410x reader interpretation (Wiegand 26 or 34), facility code and inclusive allocation range. Wiegand 26 supports facilities 0–255; Wiegand 34 supports 0–65535. Both use 16-bit card numbers; automatic allocation is constrained to your selected range within 1–65535. Confirm the interpretation against the reader configuration before saving.

No organisation or facility is assumed for new installs. The synthetic example profile uses invented values and requires confirmation when loaded. **Organisation settings** edits the names and allocation range later. Encoding and facility cannot change after cards or observations exist: use a separate register for that site instead. Existing cards outside a new allocation range are retained.

**Open organisation** switches registers. Allocation is unique within that register only, not across independent copies/PCs or a live controller. One app instance holds an exclusive lock while a register is open. Changes are saved atomically. New-file creation refuses to overwrite an existing register.

## Import or add people

Use **Add user**, or import UTF-8 CSV with these exact headers:

```csv
User Number,User Name,Card Number,Full EM410x ID,Notes
```

User Number is the controller's optional user number. Blank values remain unknown; local keys are separate. Card Number can be blank for a user without a fob. Full EM410x ID is optional but, when supplied, must match the configured facility and the row's card number. See the synthetic CSV example. Multiple rows may refer to the same user. Duplicate/reserved card numbers and conflicting user names are rejected. An invalid row cancels the whole import.

CSV import is additive. Existing records are edited through the user/fob controls. CSV export includes current assignments and retained/deleted records; it is a reporting format, not an automatic controller import or the same format as the import template.

## Manage fobs

- Select a user and **Reserve unused number for new fob**. Reservations survive cancellation, failure and restart.
- **Scan & find user** selects the current holder of a known credential. Unknown credentials require an explicit link. Wrong-facility, wrong-protocol and deleted credentials are rejected.
- **Reassign fob** records a transfer without changing its full ID or number. Previous holders and history remain retained.
- **Clone fob** first reads the selected original, then checks a destination and verifies the full ID after writing. It does not allocate a new number.
- **Edit fob** changes notes or prepares an encoded-number change. The new number must be unused and inside the allocation range. The old local assignment is retired only after the rewrite independently verifies. Failed attempts leave the old record active and the new number reserved.
- **Delete fob** removes it from the active local list after review. The number and history remain retained; the physical fob is not erased and access is not revoked automatically.

Each write requires a supported password-free destination and explicit overwrite review. Physical source and destination contexts remain separate from demo data. New/changed assignments and revocation must be applied separately in your access system.

## Private storage and migration

Store registers outside the source repository, preferably under your user profile's application-data folder. The register, exports and profile JSON are plaintext, separate from optional Library encryption. Back them up privately. **Export profile** contains only organisation configuration, without users or fobs; it is still company information and is ignored by Git.

Legacy schema 1/2 registers are upgraded to schema 3 after a uniquely named private backup is saved. Existing users, unknown controller numbers, holders, full fob IDs, reservations and history are preserved. The previous site/facility becomes the organisation profile. Older app versions reject the new schema. Never replace a live register with an older backup after further reservations.

The isolated demo always uses its own synthetic register. It cannot modify the physical register. Profile files exported from real organisations, backups and company rosters must not be committed or attached to public issues.
