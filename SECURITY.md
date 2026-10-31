# Security Policy

## Supported Versions

We release patches for security vulnerabilities. Currently supported versions:

| Version | Supported          |
| ------- | ------------------ |
| 2.x     | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them via:

1. **GitHub Security Advisories** (preferred):
   - Go to [Security Advisories](https://github.com/timwuhaotian/the-pair/security/advisories/new)
   - Submit a private vulnerability report

2. **Email** (alternative):
   - Create an issue at [GitHub Issues](https://github.com/timwuhaotian/the-pair/issues/new) indicating you have a security concern
   - A maintainer will provide a secure communication channel

### What to Include

Please include:

- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Initial response**: Within 48 hours
- **Status update**: Within 7 days
- **Resolution**: Depends on severity and complexity

### Disclosure Policy

- Please do not disclose the vulnerability publicly until a fix is released
- We will credit you in the release notes (unless you prefer to remain anonymous)

## Security Best Practices

When using The Pair:

- **API Keys**: Never commit your AI provider API keys to version control
- **Workspace**: Be aware of which directories you grant the app access to
- **Permissions**: Review the permissions requested by each Pair session
- **Updates**: Keep the app updated to the latest version

## Known Security Considerations

- The Pair executes AI-generated commands in your local environment
- Always review changes before accepting them in manual approval mode
- Use workspace-scoped permissions to limit file system access
- The app reads your opencode configuration for opencode-backed model access

Thank you for helping keep The Pair secure!
