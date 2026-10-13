// OpenCR HTMX integration helpers
document.addEventListener("htmx:afterOnLoad", function(evt) {
    var navItems = document.querySelectorAll(".nav-item");
    navItems.forEach(function(item) {
        item.classList.remove("active");
        if (item.getAttribute("hx-get") === evt.detail.xhr.responseURL) {
            item.classList.add("active");
        }
    });
});
