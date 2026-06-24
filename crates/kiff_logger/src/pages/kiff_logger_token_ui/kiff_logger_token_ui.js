frappe.pages['kiff-logger-token-ui'].on_page_load = function(wrapper) {
    let page = frappe.ui.make_app_page({
        parent: wrapper,
        title: 'Kiff Logger Token UI',
        single_column: true
    });

    let $wrapper = $(wrapper).find('.layout-main-section');
    $wrapper.empty();
    $wrapper.css({ padding: 0, overflow: 'hidden' });

    $wrapper.html(`
        <iframe src="/kiff_logger/token-ui" style="width: 100%; height: calc(100vh - 120px); border: none;"></iframe>
    `);
};
