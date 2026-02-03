# APT Repository Infrastructure

Terraform manifests for the Autonomi APT repository hosted on AWS S3 + CloudFront.

## Architecture

```
                        ┌─────────────────┐
                        │   apt-get user   │
                        └────────┬─────────┘
                                 │ HTTPS
                                 ▼
                        ┌─────────────────┐
                        │   CloudFront    │
                        │   Distribution  │
                        │   (CDN + HTTPS) │
                        │   TTL: 300s     │
                        └────────┬────────┘
                                 │ OAI
                                 ▼
                        ┌─────────────────┐
                        │   S3 Bucket     │
                        │ autonomi-apt-repo│
                        │                 │
                        │ dists/stable/   │
                        │   InRelease     │
                        │   Release.gpg   │
                        │   main/         │
                        │     binary-*/   │
                        │       Packages  │
                        │ pool/main/a/    │
                        │   autonomi/     │
                        │     *.deb       │
                        └─────────────────┘
                                 ▲
                                 │ s3 sync + CF invalidation
                        ┌─────────────────┐
                        │   IAM User      │
                        │ (CI/CD access)  │
                        │                 │
                        │ - s3:PutObject  │
                        │ - s3:GetObject  │
                        │ - s3:ListBucket │
                        │ - cf:Create-    │
                        │   Invalidation  │
                        └─────────────────┘
                                 ▲
                                 │
                        ┌─────────────────┐
                        │  GitHub Actions │
                        │  (publish-apt-  │
                        │   repo.yml)     │
                        └─────────────────┘
```

## Prerequisites

- [Terraform](https://www.terraform.io/downloads) >= 1.0
- AWS CLI configured with credentials that can create S3 buckets, CloudFront distributions, and IAM users
- An AWS account

## Usage

```bash
cd resources/terraform/apt-repo

# Initialize Terraform
terraform init

# Review the plan
terraform plan

# Apply (creates all resources)
terraform apply
```

## Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `bucket_name` | S3 bucket name | `autonomi-apt-repo` |
| `aws_region` | AWS region | `eu-west-2` |
| `cloudfront_price_class` | CDN price class | `PriceClass_100` (US/EU) |
| `cloudfront_ttl` | Default cache TTL (seconds) | `300` |
| `iam_user_name` | IAM user for CI/CD | `autonomi-apt-repo-ci` |
| `tags` | Resource tags | `Project=autonomi, Component=apt-repo` |

## Outputs

| Output | Description |
|--------|-------------|
| `bucket_name` | S3 bucket name |
| `cloudfront_distribution_id` | CloudFront ID (needed for CI cache invalidation) |
| `cloudfront_domain_name` | Domain name for apt sources.list |
| `apt_sources_line` | Complete sources.list line for users |
| `iam_user_name` | IAM user name |
| `iam_user_arn` | IAM user ARN |

## Post-Apply Steps

After `terraform apply`:

1. **Create access keys** for the IAM user:
   ```bash
   aws iam create-access-key --user-name autonomi-apt-repo-ci
   ```

2. **Add secrets to GitHub**:
   - `APT_REPO_AWS_ACCESS_KEY_ID` - from step 1
   - `APT_REPO_AWS_SECRET_ACCESS_KEY` - from step 1
   - `APT_REPO_CLOUDFRONT_DISTRIBUTION_ID` - from `terraform output cloudfront_distribution_id`
   - `APT_REPO_BUCKET_NAME` - from `terraform output bucket_name`

3. **Upload the GPG public key** to the bucket root:
   ```bash
   aws s3 cp resources/keys/autonomi-signing-key.asc \
     s3://autonomi-apt-repo/autonomi-signing-key.asc \
     --content-type application/pgp-keys
   ```

4. Note the CloudFront domain:
   ```bash
   terraform output cloudfront_domain_name
   ```
   This is the URL users will add to their `sources.list`.
