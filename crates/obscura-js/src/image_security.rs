//! Readability is a property of a successfully checked transport response,
//! including its redirect chain, not of the image element's current URL.
use obscura_net::Response;
use obscura_render::ImageRequestProfile;
use url::Url;

pub(crate) fn response_origin_clean(
    initiator: &Url,
    requested: &Url,
    profile: ImageRequestProfile,
    response: &Response,
) -> bool {
    if !(200..300).contains(&response.status) {
        return false;
    }
    // These profiles reach this function only after the browser transport's
    // credential-aware CORS check has succeeded, including every redirect.
    if profile != ImageRequestProfile::NoCorsInclude {
        return true;
    }
    if requested.scheme() == "data"
        && response.url.scheme() == "data"
        && response.redirected_from.is_empty()
    {
        return true;
    }
    let same = |url: &Url| {
        matches!(initiator.scheme(), "http" | "https")
            && matches!(url.scheme(), "http" | "https")
            && url.origin() == initiator.origin()
    };
    same(requested) && same(&response.url) && response.redirected_from.iter().all(same)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }
    fn response(final_url: &str, redirects: &[&str]) -> Response {
        Response {
            url: url(final_url),
            status: 200,
            headers: Default::default(),
            body: vec![],
            redirected_from: redirects.iter().map(|v| url(v)).collect(),
        }
    }
    #[test]
    fn no_cors_requires_the_whole_route_to_be_same_origin() {
        let owner = url("https://site.test/page");
        let requested = url("https://site.test/image");
        for (final_url, redirects, expected) in [
            ("https://site.test/final", vec![], true),
            (
                "https://site.test/final",
                vec!["https://site.test/image"],
                true,
            ),
            (
                "https://cdn.test/final",
                vec!["https://site.test/image"],
                false,
            ),
            (
                "https://site.test/final",
                vec!["https://cdn.test/hop"],
                false,
            ),
            ("http://site.test/final", vec![], false),
            ("https://site.test:444/final", vec![], false),
        ] {
            assert_eq!(
                response_origin_clean(
                    &owner,
                    &requested,
                    ImageRequestProfile::NoCorsInclude,
                    &response(final_url, &redirects)
                ),
                expected
            );
        }
        assert!(!response_origin_clean(
            &owner,
            &url("https://cdn.test/start"),
            ImageRequestProfile::NoCorsInclude,
            &response("https://site.test/end", &[])
        ));
    }
    #[test]
    fn successful_cors_responses_are_readable_but_failed_responses_are_not() {
        let owner = url("https://site.test");
        let requested = url("https://cdn.test/image");
        for profile in [
            ImageRequestProfile::CorsInclude,
            ImageRequestProfile::CorsSameOrigin,
        ] {
            let mut value = response(requested.as_str(), &[]);
            assert!(response_origin_clean(&owner, &requested, profile, &value));
            value.status = 403;
            assert!(!response_origin_clean(&owner, &requested, profile, &value));
        }
    }
    #[test]
    fn opaque_origins_are_not_equal_and_data_images_need_no_network_permission() {
        let blank = url("about:blank");
        assert!(!response_origin_clean(
            &blank,
            &blank,
            ImageRequestProfile::NoCorsInclude,
            &response("about:blank", &[])
        ));
        let data = url("data:image/png;base64,AA==");
        assert!(response_origin_clean(
            &blank,
            &data,
            ImageRequestProfile::NoCorsInclude,
            &response(data.as_str(), &[])
        ));
    }
}
