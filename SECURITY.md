# Security and privacy

This tool reads explicitly supplied local report snapshots. It does not execute tests, shell commands, report instructions, or network requests. Treat reports, test names, paths and exported Markdown as untrusted data.

XML parsers must not fetch external resources or expand internal entity definitions. Input bounds, explicit dialect validation, duplicate-identity detection and inconclusive verdicts are intentional. Do not weaken them to make a comparison pass.

Use stable, trusted report directories. Encountered symbolic links are rejected or omitted with diagnostics; standard filesystem operations do not provide a race-proof sandbox or prevent an explicitly supplied ancestor path from resolving through a symbolic link.

Exports omit captured logs, properties and exception bodies, but test names and relative paths may still identify private projects or data. Review before sharing with AI services, issue trackers or people. Never include real credentials or private reports in public issues. Use a minimal synthetic reproduction when reporting a problem; use the repository owner's private reporting channel when available.
