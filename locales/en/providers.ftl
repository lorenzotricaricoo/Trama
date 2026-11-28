# AI providers: errors of provider calls, by the codes the core returns.
# Each message id is `provider-error-` followed by the code, with `_` written as `-`.

provider-error-timeout = The provider didn't reply in time.
provider-error-network = The provider can't be reached. Check the connection and the provider's address.
provider-error-rate-limited = The provider is limiting requests. Trama will try again later.
provider-error-auth-failed = The provider refused the API key. Check the key in Settings.
provider-error-not-found = The provider doesn't know this model or address.
provider-error-bad-request = The provider refused the request.
provider-error-server-error = The provider had an error. Trama will try again later.
provider-error-invalid-response = The provider sent a reply Trama can't read.
provider-error-truncated = The reply was cut off at the model's output limit.
provider-error-content-filtered = The provider's content filter stopped the reply.
provider-error-empty-reply = The model sent an empty reply.
provider-error-invalid-json = The model's reply wasn't in the expected format.
provider-error-schema-mismatch = The model's reply was missing required information.
provider-error-degenerate-output = The model's reply was empty or placeholder text.
provider-error-context-exceeded = The text is too long for this model.
provider-error-limits-unknown = The model's context size is unknown. Set it in Settings.
provider-error-no-model-for-task = No model is chosen for this task. Choose one in Settings.
provider-error-missing-key = The provider's API key is missing. Add it in Settings.
provider-error-keychain = Trama couldn't access the API key in the Keychain.
provider-error-storage = Trama couldn't read or save the provider settings.
provider-error-key-required-for-new-url = The provider's address now points to another server. Enter the API key for it: the stored key is never sent to a different server.
provider-error-price-missing = The monthly budget is enforced, but this model has no price, so its cost can't be counted. Set its prices in Settings or turn off the limit.
provider-error-budget-exceeded = This month's budget for language models is used up. Raise it or turn off the limit in Settings.
