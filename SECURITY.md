# Security and private data

Use only readers, fobs and access systems you are authorised to administer. Do not post access credentials, authentication keys, card dumps or personal information in public issues.

Company files are plaintext local records and should be protected and backed up accordingly. Organisation mode does not connect to a live controller. Deletion in the local register does not revoke door access; cloning creates another fob with the same credential.

The project ignore rules exclude private organisation/configuration/export/backup files and local logs. Only named synthetic fixtures are exempt. Public source packaging checks these rules with Git itself. Keep real data out of those fixtures and source code. Review existing tracked files and history separately before publishing a repository.

When reporting a problem, use a synthetic reproducer and describe the application version, operating system and observed behaviour without including secrets or private records.
