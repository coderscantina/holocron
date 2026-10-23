# Run Holocron next to Umami behind CloudFront

[`index.mjs`](index.mjs) is a Lambda@Edge function that copies every tracker hit (`POST /api/send`) to Holocron while CloudFront keeps sending it to Umami. The sites keep loading Umami's tracker and nothing on them changes. Use it to compare both for a while before you [cut over](../../docs/guide/umami-migration.md#cut-over-the-tracker).

- Visitors always get Umami's answer. The function waits at most 500 ms for Holocron and ignores its answer and its failures, its own included: an error in a Lambda@Edge handler would turn into a 5xx for the visitor.
- Holocron gets the visitor's address and location from the `CloudFront-Viewer-*` headers the function passes on.
- Umami's `x-umami-cache` tokens are not passed on and Holocron's never reach the browser, so Holocron groups visits into sessions by its own rules. Session counts can differ slightly from Umami's.
- Only `/api/send` is copied. Replays and heatmaps need Holocron's recorder, which the sites don't load yet.

## Before you start

- Holocron runs and is reachable over HTTPS. It may still be in setup mode: it answers the copies with 503 until the [import](../../docs/guide/umami-migration.md) ends, and the function ignores that. Start the copy before the import, so hits flow in the moment setup ends and the gap is only the import's own run.
- Holocron trusts the proxy in front of it and reads `cloudfront-viewer-address` (`HOLOCRON_COLLECT_TRUSTED_PROXIES`, `HOLOCRON_COLLECT_CLIENT_IP_HEADER`). [deploy/coolify](../coolify/docker-compose.yml) is set up that way.

## Deploy

1. Set `HOLOCRON` at the top of `index.mjs` to Holocron's address, for example `https://analytics.example.com`. Lambda@Edge has no environment variables.
2. Create an IAM role that `lambda.amazonaws.com` and `edgelambda.amazonaws.com` may assume, with the managed policy `AWSLambdaBasicExecutionRole`.
3. In **us-east-1**, create the function and publish a version:

   ```sh
   zip mirror.zip index.mjs
   aws lambda create-function --region us-east-1 --function-name holocron-mirror \
     --runtime nodejs22.x --handler index.handler --timeout 5 --memory-size 128 \
     --role arn:aws:iam::<account>:role/<role> --zip-file fileb://mirror.zip --publish
   ```

   The output's `FunctionArn` ends in the version number, for example `:1`.
4. In the CloudFront distribution that serves Umami, add a behaviour:
   - Path pattern `/api/send`, origin: Umami's origin, the same as the default behaviour.
   - Allowed methods: GET, HEAD, OPTIONS, PUT, POST, PATCH, DELETE.
   - Cache policy `CachingDisabled`.
   - An origin request policy that forwards what the default behaviour forwards, plus `CloudFront-Viewer-Address`, `CloudFront-Viewer-Country`, `CloudFront-Viewer-Country-Region`, `CloudFront-Viewer-City`, `CloudFront-Viewer-Latitude` and `CloudFront-Viewer-Longitude`. The managed `AllViewerAndCloudFrontHeaders-2022-06` does when the default forwards all viewer headers.
   - Function association, origin request: the version ARN from step 3, with **Include body** on.
5. Wait for the distribution to deploy. Open a tracked site, then Holocron's Realtime: the visit shows up with its country. Umami shows it too.

To change the target, edit `index.mjs`, publish a new version with `aws lambda update-function-code ... --publish` and point the association at it.

## Stop

Remove the function association from the `/api/send` behaviour, or delete the behaviour. Umami is untouched either way. The function's logs are in CloudWatch, in the region closest to each visitor, under `/aws/lambda/us-east-1.holocron-mirror`.
