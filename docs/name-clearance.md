# Name clearance checklist

`curl++` is unsuitable as a public name because the long-running `curlpp`
project already uses the spoken and normalized form, package managers handle
plus signs inconsistently, and the name could imply affiliation with curl.

Verbsmith passed a preliminary exact-name recheck on 19 September 2026:

- The [GitHub repository search](https://api.github.com/search/repositories?q=verbsmith+in%3Aname)
  returned zero names.
- Exact package records were not found on [crates.io](https://crates.io/crates/verbsmith),
  [npm](https://www.npmjs.com/package/verbsmith), or
  [PyPI](https://pypi.org/project/verbsmith/).
- RDAP returned no registration for `verbsmith.dev`, `verbsmith.io`, or
  `verbsmith.co.uk`; this does not guarantee that a registrar can register them.
- A general search returned no software product using the exact name.

Search results change and package/domain availability does not establish
trademark rights. No domain, package, repository, or social handle has been
reserved by this check.

Before announcing the project:

1. Search UKIPO, EUIPO, USPTO, and WIPO records in software and hosted-service classes.
2. Repeat package, GitHub, domain, social-handle, and general web searches.
3. Obtain qualified legal review; this document is not legal advice.
4. Register the chosen domain and repository names together.
5. Use curl and competitor names only for factual compatibility statements.
6. Keep the non-affiliation statement in release documentation.
