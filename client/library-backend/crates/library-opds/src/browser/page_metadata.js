(() => {
    const icon = document.querySelector('link[rel~="icon"]');
    return {
        url: location.href,
        title: document.title,
        site_name: document.querySelector('meta[property="og:site_name"], meta[name="application-name"]')?.content || null,
        icon: icon ? icon.href : new URL('/favicon.ico', location.href).href
    };
})()
