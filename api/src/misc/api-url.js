export function createAPIURL(baseURL, pathname) {
    const url = new URL(baseURL);
    const basePath = url.pathname.replace(/\/+$/, '');
    const endpointPath = pathname.replace(/^\/+/, '');

    url.pathname = `${basePath}/${endpointPath}`;
    url.search = '';
    url.hash = '';

    return url;
}
