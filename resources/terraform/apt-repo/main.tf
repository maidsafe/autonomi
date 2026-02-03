terraform {
  required_version = ">= 1.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

# -----------------------------------------------------------------------------
# S3 Bucket
# -----------------------------------------------------------------------------

resource "aws_s3_bucket" "apt_repo" {
  bucket = var.bucket_name
  tags   = var.tags
}

resource "aws_s3_bucket_versioning" "apt_repo" {
  bucket = aws_s3_bucket.apt_repo.id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_public_access_block" "apt_repo" {
  bucket = aws_s3_bucket.apt_repo.id

  block_public_acls       = false
  block_public_policy     = false
  ignore_public_acls      = false
  restrict_public_buckets = false
}

resource "aws_s3_bucket_policy" "apt_repo_public_read" {
  bucket = aws_s3_bucket.apt_repo.id

  depends_on = [aws_s3_bucket_public_access_block.apt_repo]

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid       = "PublicReadGetObject"
        Effect    = "Allow"
        Principal = "*"
        Action    = "s3:GetObject"
        Resource  = "${aws_s3_bucket.apt_repo.arn}/*"
      }
    ]
  })
}

# -----------------------------------------------------------------------------
# CloudFront Distribution
# -----------------------------------------------------------------------------

resource "aws_cloudfront_origin_access_identity" "apt_repo" {
  comment = "OAI for ${var.bucket_name}"
}

resource "aws_cloudfront_distribution" "apt_repo" {
  enabled             = true
  is_ipv6_enabled     = true
  comment             = "Autonomi APT Repository"
  default_root_object = ""
  price_class         = var.cloudfront_price_class
  tags                = var.tags

  origin {
    domain_name = aws_s3_bucket.apt_repo.bucket_regional_domain_name
    origin_id   = "S3-${var.bucket_name}"

    s3_origin_config {
      origin_access_identity = aws_cloudfront_origin_access_identity.apt_repo.cloudfront_access_identity_path
    }
  }

  default_cache_behavior {
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    target_origin_id       = "S3-${var.bucket_name}"
    viewer_protocol_policy = "redirect-to-https"
    compress               = true

    forwarded_values {
      query_string = false

      cookies {
        forward = "none"
      }
    }

    min_ttl     = 0
    default_ttl = var.cloudfront_ttl
    max_ttl     = 86400
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    cloudfront_default_certificate = true
  }
}

# -----------------------------------------------------------------------------
# IAM User for CI/CD
# -----------------------------------------------------------------------------

resource "aws_iam_user" "ci" {
  name = var.iam_user_name
  tags = var.tags
}

resource "aws_iam_policy" "ci_apt_repo" {
  name        = "${var.iam_user_name}-policy"
  description = "Allow CI to publish to APT repo S3 bucket and invalidate CloudFront"

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "S3ReadWrite"
        Effect = "Allow"
        Action = [
          "s3:GetObject",
          "s3:PutObject",
          "s3:DeleteObject",
          "s3:ListBucket"
        ]
        Resource = [
          aws_s3_bucket.apt_repo.arn,
          "${aws_s3_bucket.apt_repo.arn}/*"
        ]
      },
      {
        Sid    = "CloudFrontInvalidation"
        Effect = "Allow"
        Action = [
          "cloudfront:CreateInvalidation",
          "cloudfront:GetInvalidation",
          "cloudfront:ListInvalidations"
        ]
        Resource = aws_cloudfront_distribution.apt_repo.arn
      }
    ]
  })
}

resource "aws_iam_user_policy_attachment" "ci_apt_repo" {
  user       = aws_iam_user.ci.name
  policy_arn = aws_iam_policy.ci_apt_repo.arn
}
