variable "bucket_name" {
  description = "Name of the S3 bucket for the APT repository"
  type        = string
  default     = "autonomi-apt-repo"
}

variable "aws_region" {
  description = "AWS region for the S3 bucket"
  type        = string
  default     = "eu-west-2"
}

variable "cloudfront_price_class" {
  description = "CloudFront price class (PriceClass_100 = US/EU only, PriceClass_200 = +Asia, PriceClass_All = global)"
  type        = string
  default     = "PriceClass_100"
}

variable "cloudfront_ttl" {
  description = "Default TTL in seconds for CloudFront cache"
  type        = number
  default     = 300
}

variable "iam_user_name" {
  description = "Name of the IAM user for CI/CD access to the APT repo"
  type        = string
  default     = "autonomi-apt-repo-ci"
}

variable "tags" {
  description = "Tags to apply to all resources"
  type        = map(string)
  default = {
    Project   = "autonomi"
    Component = "apt-repo"
    ManagedBy = "terraform"
  }
}
