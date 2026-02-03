output "bucket_name" {
  description = "Name of the S3 bucket hosting the APT repository"
  value       = aws_s3_bucket.apt_repo.id
}

output "bucket_arn" {
  description = "ARN of the S3 bucket"
  value       = aws_s3_bucket.apt_repo.arn
}

output "cloudfront_distribution_id" {
  description = "CloudFront distribution ID (needed for cache invalidation in CI)"
  value       = aws_cloudfront_distribution.apt_repo.id
}

output "cloudfront_domain_name" {
  description = "CloudFront domain name (use this in apt sources.list)"
  value       = aws_cloudfront_distribution.apt_repo.domain_name
}

output "apt_sources_line" {
  description = "The sources.list line users should add"
  value       = "deb [signed-by=/usr/share/keyrings/autonomi-archive-keyring.gpg] https://${aws_cloudfront_distribution.apt_repo.domain_name} stable main"
}

output "iam_user_name" {
  description = "Name of the IAM user for CI/CD"
  value       = aws_iam_user.ci.name
}

output "iam_user_arn" {
  description = "ARN of the IAM user for CI/CD"
  value       = aws_iam_user.ci.arn
}
