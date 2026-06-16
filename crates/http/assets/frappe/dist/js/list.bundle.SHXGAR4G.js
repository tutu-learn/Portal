(() => {
  var __create = Object.create;
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getOwnPropSymbols = Object.getOwnPropertySymbols;
  var __getProtoOf = Object.getPrototypeOf;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __propIsEnum = Object.prototype.propertyIsEnumerable;
  var __defNormalProp = (obj, key, value) => key in obj ? __defProp(obj, key, { enumerable: true, configurable: true, writable: true, value }) : obj[key] = value;
  var __spreadValues = (a, b) => {
    for (var prop in b || (b = {}))
      if (__hasOwnProp.call(b, prop))
        __defNormalProp(a, prop, b[prop]);
    if (__getOwnPropSymbols)
      for (var prop of __getOwnPropSymbols(b)) {
        if (__propIsEnum.call(b, prop))
          __defNormalProp(a, prop, b[prop]);
      }
    return a;
  };
  var __commonJS = (cb, mod) => function __require() {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
    isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
    mod
  ));
  var __publicField = (obj, key, value) => {
    __defNormalProp(obj, typeof key !== "symbol" ? key + "" : key, value);
    return value;
  };

  // frappe/public/js/frappe/views/treeview.js
  var require_treeview = __commonJS({
    "frappe/public/js/frappe/views/treeview.js"(exports, module) {
      frappe.provide("frappe.treeview_settings");
      frappe.provide("frappe.views.trees");
      window.cur_tree = null;
      frappe.views.TreeFactory = class TreeFactory extends frappe.views.Factory {
        make(route) {
          frappe.model.with_doctype(route[1], function() {
            var options = {
              doctype: route[1],
              meta: frappe.get_meta(route[1])
            };
            if (!frappe.treeview_settings[route[1]] && !frappe.meta.get_docfield(route[1], "is_group")) {
              frappe.msgprint(__("Tree view is not available for {0}", [route[1]]));
              return false;
            }
            $.extend(options, frappe.treeview_settings[route[1]] || {});
            frappe.views.trees[options.doctype] = new frappe.views.TreeView(options);
          });
        }
        on_show() {
          let route = frappe.get_route();
          let treeview = frappe.views.trees[route[1]];
          treeview && treeview.make_tree();
        }
        get view_name() {
          return "Tree";
        }
      };
      frappe.views.TreeView = class TreeView {
        constructor(opts) {
          var me2 = this;
          this.opts = {};
          this.opts.get_tree_root = true;
          this.opts.show_expand_all = true;
          $.extend(this.opts, opts);
          this.doctype = opts.doctype;
          this.args = { doctype: me2.doctype };
          this.page_name = frappe.get_route_str();
          this.get_tree_nodes = me2.opts.get_tree_nodes || "frappe.desk.treeview.get_children";
          this.get_permissions();
          this.make_page();
          this.make_filters();
          this.root_value = null;
          if (me2.opts.get_tree_root) {
            this.get_root();
          }
          this.onload();
          if (!this.opts.do_not_setup_menu) {
            this.set_menu_item();
          }
          this.set_primary_action();
        }
        get_permissions() {
          this.can_read = frappe.model.can_read(this.doctype);
          this.can_create = frappe.boot.user.can_create.indexOf(this.doctype) !== -1 || frappe.boot.user.in_create.indexOf(this.doctype) !== -1;
          this.can_write = frappe.model.can_write(this.doctype);
          this.can_delete = frappe.model.can_delete(this.doctype);
        }
        make_page() {
          var me2 = this;
          if (!this.opts || !this.opts.do_not_make_page) {
            this.parent = frappe.container.add_page(this.page_name);
            $(this.parent).addClass("treeview");
            frappe.ui.make_app_page({ parent: this.parent, single_column: true });
            this.page = this.parent.page;
            frappe.container.change_to(this.page_name);
            frappe.breadcrumbs.add(
              me2.opts.breadcrumb || locals.DocType[me2.doctype].module,
              me2.doctype
            );
            this.set_title();
            this.page.main.css({
              "min-height": "300px"
            });
            this.page.main.addClass("frappe-card");
          } else {
            this.page = this.opts.page;
            $(this.page[0]).addClass("frappe-card");
          }
          if (frappe.meta.has_field(me2.doctype, "disabled")) {
            this.page.add_inner_button(
              __("Include Disabled"),
              function() {
                me2.toggle_disable(event.target);
                me2.make_tree();
              },
              __("Expand"),
              "default",
              true
            );
          }
          if (this.opts.show_expand_all) {
            this.page.add_inner_button(
              __("Collapse All"),
              function() {
                me2.tree.load_children(me2.tree.root_node, false);
              },
              __("Expand"),
              "default",
              true
            );
            this.page.add_inner_button(
              __("Expand All"),
              function() {
                me2.tree.load_children(me2.tree.root_node, true);
              },
              __("Expand"),
              "default",
              true
            );
          }
          if (this.opts.view_template) {
            var row = $('<div class="row"><div>').appendTo(this.page.main);
            this.body = $('<div class="col-sm-6 col-xs-12"></div>').appendTo(row);
            this.node_view = $('<div class="col-sm-6 hidden-xs"></div>').appendTo(row);
          } else {
            this.body = this.page.main;
          }
        }
        set_title() {
          this.page.set_title(this.opts.title || __("{0} Tree", [__(this.doctype)]));
        }
        onload() {
          var me2 = this;
          this.opts.onload && this.opts.onload(me2);
        }
        make_filters() {
          var me2 = this;
          frappe.treeview_settings.filters = [];
          $.each(this.opts.filters || [], function(i2, filter) {
            if (frappe.route_options && frappe.route_options[filter.fieldname]) {
              filter.default = frappe.route_options[filter.fieldname];
            }
            if (!filter.disable_onchange) {
              filter.change = function() {
                filter.onchange && filter.onchange();
                var val = this.get_value();
                me2.args[filter.fieldname] = val;
                if (val) {
                  me2.root_label = val;
                } else {
                  me2.root_label = me2.opts.root_label;
                }
                me2.set_title();
                me2.make_tree();
              };
            }
            if (filter.render_on_toolbar) {
              me2.page.add_field(filter, me2.page.filters);
            } else {
              me2.page.add_field(filter);
            }
            if (filter.default) {
              $("[data-fieldname='" + filter.fieldname + "']").trigger("change");
            }
          });
        }
        get_root() {
          var me2 = this;
          frappe.call({
            method: me2.get_tree_nodes,
            args: me2.args,
            callback: function(r) {
              if (r.message) {
                if (r.message.length == 1) {
                  me2.root_label = r.message[0]["value"];
                  me2.root_value = me2.root_label;
                } else {
                  me2.root_label = me2.doctype;
                  me2.root_value = "";
                }
                me2.make_tree();
              }
            }
          });
        }
        toggle_disable(el) {
          if (this.args["include_disabled"]) {
            this.args["include_disabled"] = false;
            el.innerText = el.innerText.replace("Exclude", "Include");
          } else {
            this.args["include_disabled"] = true;
            console.log(el);
            el.innerText = el.innerText.replace("Include", "Exclude");
          }
        }
        make_tree() {
          $(this.parent).find(".tree").remove();
          var use_label = this.args[this.opts.root_label] || this.root_label || this.opts.root_label;
          var use_value = this.root_value;
          if (use_value == null) {
            use_value = use_label;
          }
          this.tree = new frappe.ui.Tree({
            parent: this.body,
            label: use_label,
            root_value: use_value,
            expandable: true,
            args: this.args,
            method: this.get_tree_nodes,
            toolbar: this.get_toolbar(),
            get_label: this.opts.get_label,
            on_render: this.opts.onrender,
            on_get_node: this.opts.on_get_node,
            on_node_render: this.opts.on_node_render,
            on_click: (node) => {
              this.select_node(node);
            }
          });
          cur_tree = this.tree;
          cur_tree.view_name = "Tree";
          this.post_render();
        }
        toggle_label() {
          console.log("hello");
        }
        rebuild_tree() {
          let me2 = this;
          frappe.call({
            method: "frappe.utils.nestedset.rebuild_tree",
            args: {
              doctype: me2.doctype
            },
            callback: function(r) {
              if (!r.exc) {
                me2.make_tree();
              }
            }
          });
        }
        post_render() {
          var me2 = this;
          me2.opts.post_render && me2.opts.post_render(me2);
        }
        select_node(node) {
          var me2 = this;
          if (this.opts.click) {
            this.opts.click(node);
          }
          if (this.opts.view_template) {
            this.node_view.empty();
            $(
              frappe.render_template(me2.opts.view_template, {
                data: node.data,
                doctype: me2.doctype
              })
            ).appendTo(this.node_view);
          }
        }
        get_toolbar() {
          var me2 = this;
          var toolbar = [
            {
              label: __(me2.can_write ? "Edit" : "Details"),
              condition: function(node) {
                return !node.is_root && me2.can_read;
              },
              click: function(node) {
                frappe.set_route("Form", me2.doctype, node.label);
              }
            },
            {
              label: __("Add Child"),
              condition: function(node) {
                return me2.can_create && node.expandable && !node.hide_add;
              },
              click: function(node) {
                me2.new_node();
              },
              btnClass: "hidden-xs"
            },
            {
              label: __("Rename"),
              condition: function(node) {
                let allow_rename = true;
                if (me2.doctype && frappe.get_meta(me2.doctype)) {
                  if (!frappe.get_meta(me2.doctype).allow_rename)
                    allow_rename = false;
                }
                return !node.is_root && me2.can_write && allow_rename;
              },
              click: function(node) {
                frappe.model.rename_doc(me2.doctype, node.label, function(new_name) {
                  node.$tree_link.find("a").text(new_name);
                  node.label = new_name;
                  me2.tree.refresh();
                });
              },
              btnClass: "hidden-xs"
            },
            {
              label: __("Delete"),
              condition: function(node) {
                return !node.is_root && me2.can_delete;
              },
              click: function(node) {
                frappe.model.delete_doc(me2.doctype, node.label, function() {
                  node.parent.remove();
                });
              },
              btnClass: "hidden-xs"
            }
          ];
          if (this.opts.toolbar && this.opts.extend_toolbar) {
            toolbar = toolbar.filter((btn) => {
              return !me2.opts.toolbar.find((d) => d["label"] == btn["label"]);
            });
            return toolbar.concat(this.opts.toolbar);
          } else if (this.opts.toolbar && !this.opts.extend_toolbar) {
            return this.opts.toolbar;
          } else {
            return toolbar;
          }
        }
        new_node() {
          var me2 = this;
          var node = me2.tree.get_selected_node();
          if (!(node && node.expandable)) {
            frappe.msgprint(__("Select a group {0} first.", [__(me2.doctype)]));
            return;
          }
          this.prepare_fields();
          var d = new frappe.ui.Dialog({
            title: __("New {0}", [__(me2.doctype)]),
            fields: me2.fields
          });
          var args = $.extend({}, me2.args);
          args["parent_" + me2.doctype.toLowerCase().replace(/ /g, "_").replace(/-/g, "_")] = me2.args["parent"];
          d.set_value("is_group", 0);
          d.set_values(args);
          d.set_primary_action(__("Create New"), function() {
            var btn = this;
            var v = d.get_values();
            if (!v)
              return;
            v.parent = node.label;
            v.doctype = me2.doctype;
            if (node.is_root) {
              v["is_root"] = node.is_root;
            } else {
              v["is_root"] = false;
            }
            d.hide();
            frappe.dom.freeze(__("Creating {0}", [me2.doctype]));
            $.extend(args, v);
            return frappe.call({
              method: me2.opts.add_tree_node || "frappe.desk.treeview.add_node",
              args,
              callback: function(r) {
                if (!r.exc) {
                  me2.tree.load_children(node);
                }
              },
              always: function() {
                frappe.dom.unfreeze();
              }
            });
          });
          d.show();
        }
        prepare_fields() {
          var me2 = this;
          this.fields = [
            {
              fieldtype: "Check",
              fieldname: "is_group",
              label: __("Is Group"),
              description: __(
                "Further sub-groups can only be created under records marked as 'Group'"
              )
            }
          ];
          if (this.opts.fields) {
            this.fields = this.opts.fields;
          }
          this.ignore_fields = this.opts.ignore_fields || [];
          var mandatory_fields = $.map(me2.opts.meta.fields, function(d) {
            return d.reqd || d.bold && !d.read_only && !!d.is_virtual ? d : null;
          });
          var opts_field_names = this.fields.map(function(d) {
            return d.fieldname;
          });
          mandatory_fields.map(function(d) {
            if ($.inArray(d.fieldname, me2.ignore_fields) === -1 && $.inArray(d.fieldname, opts_field_names) === -1) {
              me2.fields.push(d);
            }
          });
        }
        print_tree() {
          if (!frappe.model.can_print(this.doctype)) {
            frappe.msgprint(__("You are not allowed to print this report"));
            return false;
          }
          var tree = $(".tree:visible").html();
          var me2 = this;
          frappe.ui.get_print_settings(false, function(print_settings) {
            var title = __(me2.docname || me2.doctype);
            frappe.render_tree({ title, tree, print_settings });
            frappe.call({
              method: "frappe.core.doctype.access_log.access_log.make_access_log",
              args: {
                doctype: me2.doctype,
                report_name: me2.page_name,
                page: tree,
                method: "Print"
              }
            });
          });
        }
        set_primary_action() {
          var me2 = this;
          if (!this.opts.disable_add_node && this.can_create) {
            me2.page.set_primary_action(
              __("New"),
              function() {
                me2.new_node();
              },
              "add"
            );
          }
        }
        set_menu_item() {
          var me = this;
          this.menu_items = [
            {
              label: __("View List"),
              action: function() {
                frappe.set_route(["List", me.doctype, "List"]);
              }
            },
            {
              label: __("Print"),
              action: function() {
                me.print_tree();
              }
            },
            {
              label: __("Refresh"),
              action: function() {
                me.make_tree();
              }
            }
          ];
          if (frappe.user.has_role("System Manager") && frappe.meta.has_field(me.doctype, "lft") && frappe.meta.has_field(me.doctype, "rgt")) {
            this.menu_items.push({
              label: __("Rebuild Tree"),
              action: function() {
                me.rebuild_tree();
              }
            });
          }
          if (me.opts.menu_items) {
            me.menu_items.push.apply(me.menu_items, me.opts.menu_items);
          }
          $.each(me.menu_items, function(i, menu_item) {
            var has_perm = true;
            if (menu_item["condition"]) {
              has_perm = eval(menu_item["condition"]);
            }
            if (has_perm) {
              me.page.add_menu_item(menu_item["label"], menu_item["action"]);
            }
          });
        }
      };
    }
  });

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/ui/listing.html
  frappe.templates["listing"] = `<div class="frappe-list">
	<div class="list-filters" style="display: none;">
	</div>

	<div style="margin-bottom:9px" class="list-toolbar-wrapper hide">
		<div class="list-toolbar btn-group" style="display:inline-block; margin-right: 10px;">
		</div>
	</div>
    <div style="clear:both"></div>
	<div class="no-result text-center" style="display: none;">
		{%= no_result_message %}
	</div>
	<div class="result">
		<div class="list-headers"></div>
        <div class="list-loading text-center">
        	{%= frappe.messages.get_waiting_message(__("Loading") + "..." ) %}
        </div>
		<div class="result-list"></div>
	</div>
	<div class="list-paging-area">
		<div class="row">
			<div class="col-xs-6">
				<div class="btn-group btn-group-paging">
					<button type="button" class="btn btn-default btn-sm btn-info" data-value="20">20</button>
					<button type="button" class="btn btn-default btn-sm" data-value="100">100</button>
					<button type="button" class="btn btn-default btn-sm" data-value="500">500</button>
				</div>
			</div>
			<div class="col-xs-6 text-right">
				<button class="btn btn-default btn-more btn-sm">{%= _more %}...</button>
			</div>
		</div>
	</div>
</div>
`;

  // frappe/public/js/frappe/model/indicator.js
  frappe.has_indicator = function(doctype) {
    if (frappe.model.is_submittable(doctype)) {
      return true;
    } else if ((frappe.listview_settings[doctype] || {}).get_indicator || frappe.workflow.get_state_fieldname(doctype)) {
      return true;
    } else if (frappe.meta.has_field(doctype, "enabled") || frappe.meta.has_field(doctype, "disabled")) {
      return true;
    } else if (frappe.meta.has_field(doctype, "status") && frappe.get_meta(doctype).states.length) {
      return true;
    }
    return false;
  };
  frappe.get_indicator = function(doc, doctype, show_workflow_state) {
    if (doc.__unsaved) {
      return [__("Not Saved", null, doctype), "orange"];
    }
    if (!doctype)
      doctype = doc.doctype;
    let meta = frappe.get_meta(doctype);
    var workflow = frappe.workflow.workflows[doctype];
    var without_workflow = workflow ? workflow["override_status"] : true;
    var settings = frappe.listview_settings[doctype] || {};
    var is_submittable = frappe.model.is_submittable(doctype);
    let workflow_fieldname = frappe.workflow.get_state_fieldname(doctype);
    let avoid_status_override = (frappe.workflow.avoid_status_override[doctype] || []).includes(
      doc[workflow_fieldname]
    );
    if (workflow_fieldname && (!without_workflow || show_workflow_state) && !avoid_status_override) {
      var value = doc[workflow_fieldname];
      if (value) {
        let colour = "";
        if (locals["Workflow State"][value] && locals["Workflow State"][value].style) {
          colour = {
            Success: "green",
            Warning: "orange",
            Danger: "red",
            Primary: "blue",
            Inverse: "black",
            Info: "light-blue"
          }[locals["Workflow State"][value].style];
        }
        if (!colour)
          colour = "gray";
        return [__(value, null, doctype), colour, workflow_fieldname + ",=," + value];
      }
    }
    if (is_submittable && doc.docstatus == 0 && !settings.has_indicator_for_draft) {
      return [__("Draft", null, doctype), "red", "docstatus,=,0"];
    }
    if (is_submittable && doc.docstatus == 2 && !settings.has_indicator_for_cancelled) {
      return [__("Cancelled", null, doctype), "red", "docstatus,=,2"];
    }
    if (doc.status && meta && meta.states && meta.states.find((d) => d.title === doc.status)) {
      let state = meta.states.find((d) => d.title === doc.status);
      let color_class = frappe.scrub(state.color, "-");
      return [__(doc.status, null, doctype), color_class, "status,=," + doc.status];
    }
    if (settings.get_indicator) {
      var indicator = settings.get_indicator(doc);
      if (indicator)
        return indicator;
    }
    if (is_submittable && doc.docstatus == 1) {
      return [__("Submitted", null, doctype), "blue", "docstatus,=,1"];
    }
    if (doc.status) {
      return [
        __(doc.status, null, doctype),
        frappe.utils.guess_colour(doc.status),
        "status,=," + doc.status
      ];
    }
    if (frappe.meta.has_field(doctype, "enabled")) {
      if (doc.enabled) {
        return [__("Enabled", null, doctype), "blue", "enabled,=,1"];
      } else {
        return [__("Disabled", null, doctype), "grey", "enabled,=,0"];
      }
    }
    if (frappe.meta.has_field(doctype, "disabled")) {
      if (doc.disabled) {
        return [__("Disabled", null, doctype), "grey", "disabled,=,1"];
      } else {
        return [__("Enabled", null, doctype), "blue", "disabled,=,0"];
      }
    }
  };

  // frappe/public/js/frappe/ui/filters/filter.js
  frappe.ui.Filter = class {
    constructor(opts) {
      $.extend(this, opts);
      if (this.value === null || this.value === void 0) {
        this.value = "";
      }
      this.utils = frappe.ui.filter_utils;
      this.set_conditions();
      this.set_conditions_from_config();
      this.make();
    }
    set_conditions() {
      this.conditions = [
        ["=", __("Equals")],
        ["!=", __("Not Equals")],
        ["like", __("Like")],
        ["not like", __("Not Like")],
        ["in", __("In")],
        ["not in", __("Not In")],
        ["is", __("Is")],
        [">", __("Greater Than")],
        ["<", __("Less Than")],
        [">=", __("Greater Than Or Equal To")],
        ["<=", __("Less Than Or Equal To")],
        ["Between", __("Between")],
        ["Timespan", __("Timespan")]
      ];
      this.nested_set_conditions = [
        ["descendants of", __("Descendants Of")],
        ["descendants of (inclusive)", __("Descendants Of (inclusive)")],
        ["not descendants of", __("Not Descendants Of")],
        ["ancestors of", __("Ancestors Of")],
        ["not ancestors of", __("Not Ancestors Of")]
      ];
      this.conditions.push(...this.nested_set_conditions);
      this.invalid_condition_map = {
        Date: ["like", "not like"],
        Datetime: ["like", "not like", "in", "not in", "=", "!="],
        Data: ["Between", "Timespan"],
        Time: ["Between", "Timespan"],
        Select: ["like", "not like", "Between", "Timespan"],
        Link: ["Between", "Timespan", ">", "<", ">=", "<="],
        Currency: ["Between", "Timespan"],
        Color: ["Between", "Timespan"],
        Check: this.conditions.map((c) => c[0]).filter((c) => c !== "="),
        Code: ["Between", "Timespan", ">", "<", ">=", "<=", "in", "not in"],
        "HTML Editor": ["Between", "Timespan", ">", "<", ">=", "<=", "in", "not in"],
        "Markdown Editor": ["Between", "Timespan", ">", "<", ">=", "<=", "in", "not in"],
        Password: ["Between", "Timespan", ">", "<", ">=", "<=", "in", "not in"],
        Rating: ["like", "not like", "Between", "in", "not in", "Timespan"],
        Int: ["like", "not like", "Between", "in", "not in", "Timespan"],
        Float: ["like", "not like", "Between", "in", "not in", "Timespan"],
        Percent: ["like", "not like", "Between", "in", "not in", "Timespan"]
      };
      this.special_condition_labels = {
        Date: {
          "<": __("Before"),
          ">": __("After"),
          "<=": __("On or Before"),
          ">=": __("On or After")
        },
        Datetime: {
          "<": __("Before"),
          ">": __("After"),
          "<=": __("On or Before"),
          ">=": __("On or After")
        }
      };
    }
    set_conditions_from_config() {
      if (frappe.boot.additional_filters_config) {
        this.filters_config = frappe.boot.additional_filters_config;
        for (let key of Object.keys(this.filters_config)) {
          const filter = this.filters_config[key];
          this.conditions.push([key, __(filter.label)]);
          for (let fieldtype of Object.keys(this.invalid_condition_map)) {
            if (!filter.valid_for_fieldtypes.includes(fieldtype)) {
              this.invalid_condition_map[fieldtype].push(key);
            }
          }
        }
      }
    }
    make() {
      this.filter_edit_area = $(
        frappe.render_template("edit_filter", {
          conditions: this.conditions
        })
      );
      this.parent && this.filter_edit_area.appendTo(this.parent.find(".filter-edit-area"));
      this.make_select();
      this.set_events();
      this.setup();
    }
    make_select() {
      this.fieldselect = new frappe.ui.FieldSelect({
        parent: this.filter_edit_area.find(".fieldname-select-area"),
        doctype: this.parent_doctype,
        parent_doctype: this._parent_doctype,
        filter_fields: this.filter_fields,
        input_class: "input-xs",
        select: (doctype, fieldname) => {
          this.set_field(doctype, fieldname);
        }
      });
      if (this.fieldname) {
        this.fieldselect.set_value(this.doctype, this.fieldname);
      }
    }
    set_events() {
      this.filter_edit_area.find(".remove-filter").on("click", () => {
        this.remove();
        this.on_change();
      });
      this.filter_edit_area.find(".condition").change(() => {
        if (!this.field)
          return;
        let condition = this.get_condition();
        let fieldtype = null;
        if (["in", "like", "not in", "not like"].includes(condition)) {
          fieldtype = "Data";
          this.add_condition_help(condition);
        } else {
          this.filter_edit_area.find(".filter-description").empty();
        }
        if (["Select", "MultiSelect"].includes(this.field.df.fieldtype) && ["in", "not in"].includes(condition)) {
          fieldtype = "MultiSelect";
        }
        this.set_field(this.field.df.parent, this.field.df.fieldname, fieldtype, condition);
      });
    }
    setup() {
      const fieldname = this.fieldname || "name";
      return this.set_values(this.doctype, fieldname, this.condition, this.value);
    }
    setup_state(is_new) {
      let promise = Promise.resolve();
      if (is_new) {
        this.filter_edit_area.addClass("new-filter");
      } else {
        promise = this.update_filter_tag();
      }
      if (this.hidden) {
        promise.then(() => this.$filter_tag.hide());
      }
    }
    freeze() {
      this.update_filter_tag();
    }
    update_filter_tag() {
      if (this._filter_value_set) {
        return this._filter_value_set.then(() => {
          !this.$filter_tag ? this.make_tag() : this.set_filter_button_text();
          this.filter_edit_area.hide();
        });
      } else {
        return Promise.resolve();
      }
    }
    remove() {
      this.filter_edit_area.remove();
      this.field = null;
    }
    set_values(doctype, fieldname, condition, value) {
      if (this.set_field(doctype, fieldname) === false) {
        return;
      }
      if (this.field.df.original_type === "Check") {
        value = value == 1 ? "Yes" : "No";
      }
      if (condition)
        this.set_condition(condition, true);
      this._filter_value_set = Promise.resolve();
      if (["in", "not in"].includes(condition) && Array.isArray(value)) {
        value = value.some((v) => String(v).includes(",")) ? JSON.stringify(value) : value.join(",");
      }
      if (Array.isArray(value)) {
        this._filter_value_set = this.field.set_value(value);
      } else if (value !== void 0 || value !== null) {
        this._filter_value_set = this.field.set_value((value + "").trim());
      }
      return this._filter_value_set;
    }
    set_field(doctype, fieldname, fieldtype, condition) {
      let cur = {};
      if (this.field)
        for (let k in this.field.df)
          cur[k] = this.field.df[k];
      let original_docfield = (this.fieldselect.fields_by_name[doctype] || {})[fieldname];
      if (!original_docfield) {
        console.warn(`Field ${fieldname} is not selectable.`);
        this.remove();
        return false;
      }
      let df = copy_dict(original_docfield);
      df.read_only = 0;
      df.hidden = 0;
      df.is_filter = true;
      delete df.hidden_due_to_dependency;
      let c = condition ? condition : this.utils.get_default_condition(df);
      this.set_condition(c);
      this.utils.set_fieldtype(df, fieldtype, this.get_condition());
      if (this.field && cur.fieldname == fieldname && df.fieldtype == cur.fieldtype && df.parent == cur.parent && df.options == cur.options) {
        return;
      }
      this.fieldselect.selected_doctype = doctype;
      this.fieldselect.selected_fieldname = fieldname;
      if (this.filters_config && this.filters_config[condition] && this.filters_config[condition].valid_for_fieldtypes.includes(df.fieldtype)) {
        let args = {};
        if (this.filters_config[condition].depends_on) {
          const field_name = this.filters_config[condition].depends_on;
          const filter_value = this.filter_list.get_filter_value(field_name);
          args[field_name] = filter_value;
        }
        let setup_field = (field) => {
          df.fieldtype = field.fieldtype;
          df.options = field.options;
          df.fieldname = fieldname;
          this.make_field(df, cur.fieldtype);
        };
        if (this.filters_config[condition].data) {
          let field = this.filters_config[condition].data;
          setup_field(field);
        } else {
          frappe.xcall(this.filters_config[condition].get_field, args).then((field) => {
            this.filters_config[condition].data = field;
            setup_field(field);
          });
        }
      } else {
        this.make_field(df, cur.fieldtype);
      }
    }
    make_field(df, old_fieldtype) {
      let old_text = this.field ? this.field.get_value() : null;
      this.hide_invalid_conditions(df.fieldtype, df.original_type);
      this.set_special_condition_labels(df.original_type);
      this.toggle_nested_set_conditions(df);
      let field_area = this.filter_edit_area.find(".filter-field").empty().get(0);
      df.input_class = "input-xs";
      let f = frappe.ui.form.make_control({
        df,
        parent: field_area,
        only_input: true
      });
      f.refresh();
      this.field = f;
      if (old_text && f.fieldtype === old_fieldtype) {
        this.field.set_value(old_text);
      }
      if (Array.isArray(old_text) && df.fieldtype !== old_fieldtype) {
        this.field.set_value(this.value);
      }
      this.bind_filter_field_events();
    }
    bind_filter_field_events() {
      this.field.$input.on("focusout", () => this.on_change());
      $(this.field.wrapper).find(":input").keydown((e) => {
        if (e.which == 13 && this.field.df.fieldtype !== "MultiSelect") {
          this.on_change();
        }
      });
    }
    get_value() {
      return [
        this.fieldselect.selected_doctype,
        this.field.df.fieldname,
        this.get_condition(),
        this.get_selected_value()
      ];
    }
    get_selected_value() {
      return this.utils.get_selected_value(this.field, this.get_condition());
    }
    get_selected_label() {
      return this.utils.get_selected_label(this.field);
    }
    get_condition() {
      return this.filter_edit_area.find(".condition").val();
    }
    set_condition(condition, trigger_change = false) {
      let $condition_field = this.filter_edit_area.find(".condition");
      $condition_field.val(condition);
      if (trigger_change)
        $condition_field.change();
    }
    add_condition_help(condition) {
      const description = ["in", "not in"].includes(condition) ? __("values separated by commas") : __("use % as wildcard");
      this.filter_edit_area.find(".filter-description").html(description);
    }
    make_tag() {
      if (!this.field)
        return;
      this.$filter_tag = this.get_filter_tag_element().insertAfter(
        this.parent.find(".active-tag-filters .clear-filters")
      );
      this.set_filter_button_text();
      this.bind_tag();
    }
    bind_tag() {
      this.$filter_tag.find(".remove-filter").on("click", this.remove.bind(this));
      let filter_button = this.$filter_tag.find(".toggle-filter");
      filter_button.on("click", () => {
        filter_button.closest(".tag-filters-area").find(".filter-edit-area").show();
        this.filter_edit_area.toggle();
      });
    }
    set_filter_button_text() {
      this.$filter_tag.find(".toggle-filter").html(this.get_filter_button_text());
    }
    get_filter_button_text() {
      let value = this.utils.get_formatted_value(
        this.field,
        this.get_selected_label() || this.get_selected_value()
      );
      return `${__(this.field.df.label)} ${__(this.get_condition())} ${__(value)}`;
    }
    get_filter_tag_element() {
      return $(`<div class="filter-tag btn-group">
			<button class="btn btn-default btn-xs toggle-filter"
				title="${__("Edit Filter")}">
			</button>
			<button class="btn btn-default btn-xs remove-filter"
				title="${__("Remove Filter")}">
				${frappe.utils.icon("close")}
			</button>
		</div>`);
    }
    hide_invalid_conditions(fieldtype, original_type) {
      let invalid_conditions = this.invalid_condition_map[original_type] || this.invalid_condition_map[fieldtype] || [];
      for (let condition of this.conditions) {
        this.filter_edit_area.find(`.condition option[value="${condition[0]}"]`).toggle(!invalid_conditions.includes(condition[0]));
      }
    }
    set_special_condition_labels(original_type) {
      let special_conditions = this.special_condition_labels[original_type] || {};
      for (let condition of this.conditions) {
        let special_label = special_conditions[condition[0]];
        if (special_label) {
          this.filter_edit_area.find(`.condition option[value="${condition[0]}"]`).text(special_label);
        } else {
          this.filter_edit_area.find(`.condition option[value="${condition[0]}"]`).text(__(condition[1]));
        }
      }
    }
    toggle_nested_set_conditions(df) {
      let show_condition = df.fieldtype === "Link" && frappe.boot.nested_set_doctypes.includes(df.options);
      this.nested_set_conditions.forEach((condition) => {
        this.filter_edit_area.find(`.condition option[value="${condition[0]}"]`).toggle(show_condition);
      });
    }
  };
  frappe.ui.filter_utils = {
    get_formatted_value(field, value) {
      if (field.df.fieldname === "docstatus") {
        value = { 0: "Draft", 1: "Submitted", 2: "Cancelled" }[value] || value;
      } else if (field.df.original_type === "Check") {
        value = { 0: "No", 1: "Yes" }[cint(value)];
      }
      return frappe.format(value, field.df, { only_value: 1 });
    },
    get_selected_value(field, condition) {
      var _a3;
      if (!field)
        return;
      let val = (_a3 = field.get_value()) != null ? _a3 : field.value;
      if (!val && ["Link", "Dynamic Link"].includes(field.df.fieldtype)) {
        val = field.value;
      }
      if (typeof val === "string") {
        val = strip(val);
      }
      if (condition == "is" && !val) {
        val = field.df.options[0].value;
      }
      if (field.df.original_type == "Check") {
        val = val == "Yes" ? 1 : 0;
      }
      if (["like", "not like"].includes(condition)) {
        if (val && !(val.startsWith("%") || val.endsWith("%"))) {
          val = "%" + val + "%";
        }
      } else if (["in", "not in"].includes(condition)) {
        if (val) {
          try {
            const parsed = JSON.parse(val);
            val = Array.isArray(parsed) ? parsed : [String(parsed)];
          } catch (e) {
            val = val.split(",").map((v) => strip(v)).filter((v) => v != null && v !== "");
          }
        }
      } else if (frappe.boot.additional_filters_config[condition]) {
        val = field.value || val;
      }
      if (val === "%") {
        val = "";
      }
      return val;
    },
    get_selected_label(field) {
      if (["Link", "Dynamic Link"].includes(field.df.fieldtype)) {
        return field.get_label_value();
      }
    },
    get_default_condition(df) {
      const meta = frappe.get_meta(df.parent);
      if (df.fieldtype == "Data" && !(meta == null ? void 0 : meta.is_large_table)) {
        return "like";
      } else if (df.fieldtype == "Date" || df.fieldtype == "Datetime") {
        return "Between";
      } else {
        return "=";
      }
    },
    set_fieldtype(df, fieldtype, condition) {
      if (df.original_type)
        df.fieldtype = df.original_type;
      else
        df.original_type = df.fieldtype;
      df.description = "";
      df.reqd = 0;
      df.length = 1e3;
      df.ignore_link_validation = true;
      if (fieldtype) {
        df.fieldtype = fieldtype;
        return;
      }
      if (df.fieldname == "docstatus") {
        df.fieldtype = "Select";
        df.options = [
          { value: 0, label: __("Draft") },
          { value: 1, label: __("Submitted") },
          { value: 2, label: __("Cancelled") }
        ];
      } else if (df.fieldtype == "Check") {
        df.fieldtype = "Select";
        df.options = [
          { label: __("Yes", null, "Checkbox is checked"), value: "Yes" },
          { label: __("No", null, "Checkbox is not checked"), value: "No" }
        ];
      } else if ([
        "Text",
        "Small Text",
        "Text Editor",
        "Code",
        "Attach",
        "Attach Image",
        "Markdown Editor",
        "HTML Editor",
        "Tag",
        "Phone",
        "JSON",
        "Comments",
        "Barcode",
        "Dynamic Link",
        "Read Only",
        "Assign",
        "Color"
      ].indexOf(df.fieldtype) != -1) {
        df.fieldtype = "Data";
      } else if (df.fieldtype == "Link" && [
        "=",
        "!=",
        "descendants of",
        "descendants of (inclusive)",
        "ancestors of",
        "not descendants of",
        "not ancestors of"
      ].indexOf(condition) == -1) {
        df.fieldtype = "Data";
      }
      if (df.fieldtype === "Data" && (df.options || "").toLowerCase() === "email") {
        df.options = null;
      }
      if (condition == "Between" && (df.fieldtype == "Date" || df.fieldtype == "Datetime")) {
        df.fieldtype = "DateRange";
      }
      if (condition == "Timespan" && ["Date", "Datetime", "DateRange", "Select"].includes(df.fieldtype)) {
        df.fieldtype = "Select";
        df.options = this.get_timespan_options([
          "Last",
          "Yesterday",
          "Today",
          "Tomorrow",
          "This",
          "Next"
        ]);
      }
      if (condition === "is") {
        df.fieldtype = "Select";
        df.options = [
          { label: __("Set", null, "Field value is set"), value: "set" },
          { label: __("Not Set", null, "Field value is not set"), value: "not set" }
        ];
      }
      return;
    },
    get_timespan_options(periods) {
      const last_options = [
        {
          label: __("Last 7 Days"),
          value: "last 7 days"
        },
        {
          label: __("Last 14 Days"),
          value: "last 14 days"
        },
        {
          label: __("Last 30 Days"),
          value: "last 30 days"
        },
        {
          label: __("Last 90 Days"),
          value: "last 90 days"
        },
        {
          label: __("Last Week"),
          value: "last week"
        },
        {
          label: __("Last Month"),
          value: "last month"
        },
        {
          label: __("Last Quarter"),
          value: "last quarter"
        },
        {
          label: __("Last 6 Months"),
          value: "last 6 months"
        },
        {
          label: __("Last Year"),
          value: "last year"
        }
      ];
      const this_options = [
        {
          label: __("This Week"),
          value: "this week"
        },
        {
          label: __("This Month"),
          value: "this month"
        },
        {
          label: __("This Quarter"),
          value: "this quarter"
        },
        {
          label: __("This Year"),
          value: "this year"
        }
      ];
      const next_options = [
        {
          label: __("Next 7 Days"),
          value: "next 7 days"
        },
        {
          label: __("Next 14 Days"),
          value: "next 14 days"
        },
        {
          label: __("Next 30 Days"),
          value: "next 30 days"
        },
        {
          label: __("Next Week"),
          value: "next week"
        },
        {
          label: __("Next Month"),
          value: "next month"
        },
        {
          label: __("Next Quarter"),
          value: "next quarter"
        },
        {
          label: __("Next 6 Months"),
          value: "next 6 months"
        },
        {
          label: __("Next Year"),
          value: "next year"
        }
      ];
      const options = [];
      for (const period of periods) {
        switch (period) {
          case "Last":
            options.push(...last_options);
            break;
          case "This":
            options.push(...this_options);
            break;
          case "Next":
            options.push(...next_options);
            break;
          case "Yesterday":
            options.push({
              label: __("Yesterday"),
              value: "yesterday"
            });
            break;
          case "Today":
            options.push({
              label: __("Today"),
              value: "today"
            });
            break;
          case "Tomorrow":
            options.push({
              label: __("Tomorrow"),
              value: "tomorrow"
            });
            break;
          default:
            options.push({
              label: __(period),
              value: `${period.toLowerCase()}`
            });
            break;
        }
      }
      return options;
    }
  };

  // frappe/public/js/frappe/ui/filters/filter_list.js
  frappe.ui.FilterGroup = class {
    constructor(opts) {
      $.extend(this, opts);
      this.filters = this.filters || [];
      window.fltr = this;
      if (!this.filter_button) {
        this.wrapper = this.parent;
        this.wrapper.append(this.get_filter_area_template());
        this.set_filter_events();
      } else {
        this.make_popover();
      }
    }
    make_popover() {
      this.init_filter_popover();
      this.set_clear_all_filters_event();
      this.set_popover_events();
    }
    set_clear_all_filters_event() {
      if (!this.filter_x_button)
        return;
      this.filter_x_button.on("click", () => {
        this.toggle_empty_filters(true);
        if (typeof this.base_list !== "undefined") {
          this.base_list.filter_area.clear();
        } else {
          this.clear_filters();
        }
        this.update_filter_button();
      });
    }
    hide_popover() {
      var _a3;
      (_a3 = this.filter_button) == null ? void 0 : _a3.popover("hide");
    }
    init_filter_popover() {
      this.filter_button.popover({
        content: this.get_filter_area_template(),
        template: `
				<div class="filter-popover popover">
					<div class="arrow"></div>
					<div class="popover-body popover-content">
					</div>
				</div>
			`,
        html: true,
        trigger: "manual",
        container: "body",
        placement: "bottom",
        offset: "-100px, 0"
      });
    }
    toggle_empty_filters(show) {
      this.wrapper && this.wrapper.find(".empty-filters").toggle(show);
    }
    set_popover_events() {
      $(document.body).on("mousedown", (e) => {
        if (this.wrapper && this.wrapper.is(":visible")) {
          const in_datepicker = $(e.target).is(".datepicker--cell") || $(e.target).closest(".datepicker--nav-title").length !== 0 || $(e.target).parents(".datepicker--nav-action").length !== 0 || $(e.target).parents(".datepicker").length !== 0 || $(e.target).is(".datepicker--button");
          if ($(e.target).parents(".filter-popover").length === 0 && $(e.target).parents(".filter-box").length === 0 && this.filter_button.find($(e.target)).length === 0 && !$(e.target).is(this.filter_button) && !in_datepicker) {
            this.wrapper && this.hide_popover();
          }
        }
      });
      this.filter_button.on("click", () => {
        this.filter_button.popover("toggle");
      });
      this.filter_button.on("shown.bs.popover", () => {
        let hide_empty_filters = this.filters && this.filters.length > 0;
        if (!this.wrapper) {
          this.wrapper = $(".filter-popover");
          if (hide_empty_filters) {
            this.toggle_empty_filters(false);
            this.add_filters_to_popover(this.filters);
          }
          this.set_filter_events();
        }
        this.toggle_empty_filters(false);
        !hide_empty_filters && this.add_filter(this.doctype, "name");
      });
      this.filter_button.on("hidden.bs.popover", () => {
        this.apply();
      });
      frappe.router.on("change", () => {
        if (this.wrapper && this.wrapper.is(":visible")) {
          this.hide_popover();
        }
      });
    }
    add_filters_to_popover(filters) {
      filters.forEach((filter) => {
        filter.parent = this.wrapper;
        filter.field = null;
        filter.make();
      });
    }
    apply() {
      this.update_filters();
      this.on_change();
    }
    update_filter_button() {
      const filters_applied = this.filters.length > 0;
      const button_label = filters_applied ? __("Filters {0}", [`<span class="filter-label">${this.filters.length}</span>`]) : __("Filter");
      this.filter_button.toggleClass("btn-default", !filters_applied).toggleClass("btn-primary-light", filters_applied);
      this.filter_button.find(".filter-icon").toggleClass("active", filters_applied);
      this.filter_button.find(".button-label").html(button_label);
      this.filter_button.attr(
        "title",
        `${this.filters.length} Filter${this.filters.length > 1 ? "s" : ""} Applied`
      );
    }
    set_filter_events() {
      this.wrapper.find(".add-filter").on("click", () => {
        this.toggle_empty_filters(false);
        this.add_filter(this.doctype, "name");
      });
      this.wrapper.find(".clear-filters").on("click", () => {
        this.toggle_empty_filters(true);
        this.clear_filters();
        this.on_change();
        this.hide_popover();
      });
      this.wrapper.find(".apply-filters").on("click", () => this.hide_popover());
    }
    add_filters(filters) {
      let promises = [];
      for (const filter of filters) {
        promises.push(() => this.add_filter(...filter));
      }
      return frappe.run_serially(promises).then(() => this.update_filters());
    }
    add_filter(doctype, fieldname, condition, value, hidden) {
      if (!fieldname)
        return Promise.resolve();
      if (!this.validate_args(doctype, fieldname))
        return false;
      const is_new_filter = arguments.length < 2;
      if (is_new_filter && this.wrapper.find(".new-filter:visible").length) {
        return Promise.resolve();
      } else {
        let args = [doctype, fieldname, condition, value, hidden];
        const promise = this.push_new_filter(args, is_new_filter);
        return promise && promise.then ? promise : Promise.resolve();
      }
    }
    validate_args(doctype, fieldname) {
      if (doctype && fieldname && !frappe.meta.has_field(doctype, fieldname) && frappe.model.is_non_std_field(fieldname)) {
        frappe.msgprint({
          message: __("Invalid filter: {0}", [fieldname.bold()]),
          indicator: "red"
        });
        return false;
      }
      return true;
    }
    push_new_filter(args) {
      if (this.filter_exists(args))
        return;
      let filter = this._push_new_filter(...args);
      if (filter && filter.value) {
        return filter._filter_value_set;
      }
    }
    _push_new_filter(doctype, fieldname, condition, value, hidden = false) {
      let args = {
        parent: this.wrapper,
        parent_doctype: this.doctype,
        doctype,
        _parent_doctype: this.parent_doctype,
        fieldname,
        condition,
        value,
        hidden,
        index: this.filters.length + 1,
        on_change: (update) => {
          if (update)
            this.update_filters();
          this.on_change();
        },
        filter_items: (doctype2, fieldname2) => {
          return !this.filter_exists([doctype2, fieldname2]);
        },
        filter_list: this.base_list || this
      };
      let filter = new frappe.ui.Filter(args);
      this.filters.push(filter);
      return filter;
    }
    get_filter_value(fieldname) {
      let filter_obj = this.filters.find((f) => f.fieldname == fieldname) || {};
      return filter_obj.value;
    }
    filter_exists(filter_value) {
      return this.filters.filter((f) => f.field).some((f) => {
        let f_value = f.get_value();
        if (filter_value.length === 2) {
          return filter_value[0] === f_value[0] && filter_value[1] === f_value[1];
        }
        return frappe.utils.arrays_equal(f_value.slice(0, 4), filter_value.slice(0, 4));
      });
    }
    get_filters() {
      return this.filters.filter((f) => f.field).filter((f) => f.get_selected_value() != null).map((f) => {
        return f.get_value();
      });
    }
    update_filters() {
      const filter_exists = (f) => ![void 0, null].includes(f.get_selected_value());
      this.filters.map((f) => !filter_exists(f) && f.remove());
      this.filters = this.filters.filter((f) => filter_exists(f) && f.field);
      this.update_filter_button();
      this.filters.length === 0 && this.toggle_empty_filters(true);
    }
    clear_filters() {
      this.filters.map((f) => f.remove(true));
      this.filters = [];
    }
    get_filter(fieldname) {
      return this.filters.filter((f) => {
        return f.field && f.field.df.fieldname == fieldname;
      })[0];
    }
    get_filter_area_template() {
      return $(`
			<div class="filter-area">
				<div class="filter-edit-area">
					<div class="text-muted empty-filters text-center">
						${__("No filters selected")}
					</div>
				</div>
				<hr class="divider"></hr>
				<div class="filter-action-buttons mt-2">
					<button class="text-muted add-filter btn btn-xs">
						+ ${__("Add a Filter")}
					</button>
					<div>
						<button class="btn btn-secondary btn-xs clear-filters">
							${__("Clear Filters")}
						</button>
						${this.filter_button ? `<button class="btn btn-primary btn-xs apply-filters">
								${__("Apply Filters")}
							</button>` : ""}
					</div>
				</div>
			</div>`);
    }
    get_filters_as_object() {
      return this.get_filters().reduce((acc, filter) => {
        return Object.assign(acc, {
          [filter[1]]: [filter[2], filter[3]]
        });
      }, {});
    }
    add_filters_to_filter_group(filters) {
      if (filters && filters.length) {
        this.toggle_empty_filters(false);
        filters.forEach((filter) => {
          this.add_filter(filter[0], filter[1], filter[2], filter[3]);
        });
      }
    }
    add(filters, refresh = true) {
      if (!filters || Array.isArray(filters) && filters.length === 0)
        return Promise.resolve();
      if (typeof filters[0] === "string") {
        const filter = Array.from(arguments);
        filters = [filter];
      }
      filters = filters.filter((f) => {
        return !this.exists(f);
      });
      const { non_standard_filters, promise } = this.set_standard_filter(filters);
      return promise.then(() => {
        return non_standard_filters.length > 0 && this.filter_list.add_filters(non_standard_filters);
      }).then(() => {
        refresh && this.list_view.refresh();
      });
    }
  };

  // frappe/public/js/frappe/ui/filters/field_select.js
  frappe.ui.FieldSelect = class FieldSelect {
    constructor(opts) {
      var me2 = this;
      $.extend(this, opts);
      this.fields_by_name = {};
      this.options = [];
      this.$input = $('<input class="form-control">').appendTo(this.parent).on("click", function() {
        $(this).select();
      });
      this.input_class && this.$input.addClass(this.input_class);
      this.select_input = this.$input.get(0);
      this.awesomplete = new Awesomplete(this.select_input, {
        minChars: 0,
        maxItems: 99,
        autoFirst: true,
        list: me2.options,
        item(item) {
          return $(repl('<li class="filter-field-select"><p>%(label)s</p></li>', item)).data("item.autocomplete", item).get(0);
        }
      });
      this.$input.on("awesomplete-select", function(e) {
        var o = e.originalEvent;
        var value = o.text.value;
        var item = me2.awesomplete.get_item(value);
        me2.selected_doctype = item.doctype;
        me2.selected_fieldname = item.fieldname;
        if (me2.select)
          me2.select(item.doctype, item.fieldname);
      });
      this.$input.on("awesomplete-selectcomplete", function(e) {
        var o = e.originalEvent;
        var value = o.text.value;
        var item = me2.awesomplete.get_item(value);
        me2.$input.val(item.label);
      });
      if (this.filter_fields) {
        for (var i2 in this.filter_fields)
          this.add_field_option(this.filter_fields[i2]);
      } else {
        this.build_options();
      }
      this.set_value(this.doctype, "name");
    }
    get_value() {
      return this.selected_doctype ? this.selected_doctype + "." + this.selected_fieldname : null;
    }
    val(value) {
      if (value === void 0) {
        return this.get_value();
      } else {
        this.set_value(value);
      }
    }
    clear() {
      this.selected_doctype = null;
      this.selected_fieldname = null;
      this.$input.val("");
    }
    set_value(doctype, fieldname) {
      var me2 = this;
      this.clear();
      if (!doctype)
        return;
      if (doctype.indexOf(".") !== -1) {
        var parts = doctype.split(".");
        doctype = parts[0];
        fieldname = parts[1];
      }
      $.each(this.options, function(i2, v) {
        if (v.doctype === doctype && v.fieldname === fieldname) {
          me2.selected_doctype = doctype;
          me2.selected_fieldname = fieldname;
          me2.$input.val(v.label);
          return false;
        }
      });
    }
    build_options() {
      var me2 = this;
      me2.table_fields = [];
      var std_filters = $.map(frappe.model.std_fields, function(d) {
        var opts = { parent: me2.doctype };
        if (d.fieldname == "name")
          opts.options = me2.doctype;
        return $.extend(copy_dict(d), opts);
      });
      var doctype_obj = frappe.get_meta(me2.doctype);
      if (doctype_obj && cint(doctype_obj.istable)) {
        std_filters = std_filters.concat([
          {
            fieldname: "parent",
            fieldtype: "Data",
            label: "Parent",
            parent: me2.doctype
          }
        ]);
      }
      if (this.with_blank) {
        this.options.push({
          label: "",
          value: ""
        });
      }
      var main_table_fields = std_filters.concat(frappe.meta.docfield_list[me2.doctype]);
      $.each(frappe.utils.sort(main_table_fields, "label", "string"), function(i2, df) {
        if (df.is_virtual) {
          return;
        }
        let doctype = frappe.get_meta(me2.doctype).istable && me2.parent_doctype ? me2.parent_doctype : me2.doctype;
        if (frappe.perm.has_perm(doctype, df.permlevel, "read"))
          me2.add_field_option(df);
      });
      $.each(me2.table_fields, function(i2, table_df) {
        if (table_df.options && !table_df.is_virtual) {
          let child_table_fields = [].concat(frappe.meta.docfield_list[table_df.options]);
          if (table_df.fieldtype === "Table MultiSelect") {
            const link_field = frappe.meta.get_docfields(table_df.options).find((df) => df.fieldtype === "Link");
            child_table_fields = link_field ? [link_field] : [];
          }
          $.each(frappe.utils.sort(child_table_fields, "label", "string"), function(i3, df) {
            let doctype = frappe.get_meta(me2.doctype).istable && me2.parent_doctype ? me2.parent_doctype : me2.doctype;
            if (frappe.perm.has_perm(doctype, df.permlevel, "read"))
              me2.add_field_option(df);
          });
        }
      });
    }
    add_field_option(df) {
      let me2 = this;
      if (df.fieldname == "docstatus" && !frappe.model.is_submittable(me2.doctype))
        return;
      if (frappe.model.table_fields.includes(df.fieldtype)) {
        me2.table_fields.push(df);
        return;
      }
      let label = null;
      let table = null;
      if (me2.doctype && df.parent == me2.doctype) {
        label = __(df.label, null, df.parent);
        table = me2.doctype;
      } else {
        label = __(df.label, null, df.parent) + " (" + __(df.parent) + ")";
        table = df.parent;
      }
      if (frappe.model.no_value_type.indexOf(df.fieldtype) == -1 && !(me2.fields_by_name[df.parent] && me2.fields_by_name[df.parent][df.fieldname])) {
        this.options.push({
          label,
          value: table + "." + df.fieldname,
          fieldname: df.fieldname,
          doctype: df.parent
        });
        if (!me2.fields_by_name[df.parent])
          me2.fields_by_name[df.parent] = {};
        me2.fields_by_name[df.parent][df.fieldname] = df;
      }
    }
  };

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/ui/filters/edit_filter.html
  frappe.templates["edit_filter"] = `<div class="filter-box">
	<div class="visible-xs flex justify-flex-end">
		<span class="remove-filter">
			{{ __("Remove") }}
		</span>
	</div>
	<div class="list_filter row">
		<div class="fieldname-select-area col-sm-4 ui-front form-group"></div>
		<div class="col-sm-3 form-group">
			<select class="condition form-control input-xs">
				{% for condition in conditions %}
				<option value="{{condition[0]}}">{{ condition[1] }}</option>
				{% endfor %}
			</select>
		</div>
		<div class="col-sm-4 form-group">
			<div class="filter-field"></div>
			<div class="text-muted small filter-description"></div>
		</div>
		<div class="remove-filter col-sm-1 text-center hidden-xs">
			<svg class="icon icon-sm">
				<use href="#icon-close" class="close"></use>
			</svg>
		</div>
	</div>
</div>
`;

  // frappe/public/js/frappe/ui/tags.js
  frappe.ui.Tags = class {
    constructor({ parent, placeholder, tagsList, onTagAdd, onTagRemove, onTagClick, onChange }) {
      this.tagsList = tagsList || [];
      this.onTagAdd = onTagAdd;
      this.onTagRemove = onTagRemove;
      this.onTagClick = onTagClick;
      this.onChange = onChange;
      this.setup(parent, placeholder);
    }
    setup(parent, placeholder) {
      this.$ul = parent;
      this.$input = $(`<input class="tags-input form-control mt-2"></input>`);
      this.$inputWrapper = this.get_list_element(this.$input);
      this.$placeholder = $(`<button class="add-tags-btn text-muted btn btn-link icon-btn" id="add_tags">
				${__(placeholder)}
			</button>`);
      this.$placeholder.appendTo(this.$ul.find(".form-sidebar-items"));
      this.$inputWrapper.appendTo(this.$ul);
      this.deactivate();
      this.bind();
      this.boot();
    }
    bind() {
      const me2 = this;
      const select_tag = function() {
        const tagValue = frappe.utils.xss_sanitise(me2.$input.val());
        me2.addTag(tagValue);
        me2.$input.val("");
      };
      const activate_input = () => {
        this.activate();
        this.$input.focus();
      };
      this.$input.keypress((e) => {
        if (e.which == 13 || e.keyCode == 13) {
          this.$input.trigger("enter-pressed-in-addtag");
        }
      });
      this.$input.focusout(select_tag);
      this.$input.on("input-selected", () => {
        select_tag();
        this.deactivate();
      });
      this.$input.on("blur", () => {
        this.deactivate();
      });
      this.$placeholder.on("click", activate_input);
      this.$ul.find(".tags-label").on("click", activate_input);
    }
    boot() {
      this.addTags(this.tagsList);
    }
    activate() {
      this.$placeholder.hide();
      this.$inputWrapper.show();
    }
    deactivate() {
      this.$inputWrapper.hide();
      this.$placeholder.show();
    }
    addTag(label) {
      if (label && label !== "" && !this.tagsList.includes(label)) {
        let $tag = this.get_tag(label);
        let row = this.get_list_element($tag, "form-tag-row");
        row.insertAfter(this.$inputWrapper);
        this.tagsList.push(label);
        this.onTagAdd && this.onTagAdd(label);
      }
    }
    removeTag(label) {
      label = frappe.utils.xss_sanitise(label);
      if (this.tagsList.includes(label)) {
        this.tagsList.splice(this.tagsList.indexOf(label), 1);
        this.onTagRemove && this.onTagRemove(label);
      }
    }
    addTags(labels) {
      labels.map(this.addTag.bind(this));
    }
    clearTags() {
      this.$ul.find(".form-tag-row").remove();
      this.tagsList = [];
    }
    get_list_element($element, class_name = "") {
      let $li = $(`<div class="${class_name}"></div>`);
      $element.appendTo($li);
      return $li;
    }
    get_tag(label) {
      let colored = true;
      let $tag = frappe.get_data_pill(
        label,
        label,
        (target, pill_wrapper) => {
          this.removeTag(target);
          pill_wrapper.closest(".form-tag-row").remove();
        },
        null,
        colored
      );
      if (this.onTagClick) {
        $tag.on("click", ".pill-label", () => {
          this.onTagClick(label);
        });
      }
      return $tag;
    }
  };

  // frappe/public/js/frappe/ui/tag_editor.js
  frappe.ui.TagEditor = class TagEditor {
    constructor(opts) {
      $.extend(this, opts);
      this.setup_tags();
      if (!this.user_tags) {
        this.user_tags = "";
      }
      this.initialized = true;
      this.refresh(this.user_tags);
    }
    update_user_tags(tags_string) {
      this.user_tags = tags_string;
      frappe.model.set_value(this.frm.doctype, this.frm.docname, "_user_tags", this.user_tags);
      this.on_change && this.on_change(this.user_tags);
      frappe.tags.utils.fetch_tags();
    }
    setup_tags() {
      var me2 = this;
      if (!this.parent) {
        return;
      }
      this.wrapper = this.parent;
      if (!this.wrapper.length)
        return;
      this.tags = new frappe.ui.Tags({
        parent: this.wrapper,
        placeholder: '<svg class="es-icon icon-sm"><use href="#es-line-add"></use></svg>',
        onTagAdd: (tag) => {
          if (me2.initialized && !me2.refreshing) {
            return frappe.call({
              method: "frappe.desk.doctype.tag.tag.add_tag",
              args: me2.get_args(tag),
              callback: function(r) {
                var user_tags = me2.user_tags ? me2.user_tags.split(",") : [];
                user_tags.push(tag);
                me2.update_user_tags(user_tags.join(","));
              }
            });
          }
        },
        onTagRemove: (tag) => {
          if (!me2.refreshing) {
            return frappe.call({
              method: "frappe.desk.doctype.tag.tag.remove_tag",
              args: me2.get_args(tag),
              callback: function(r) {
                var user_tags = me2.user_tags.split(",");
                user_tags.splice(user_tags.indexOf(tag), 1);
                me2.update_user_tags(user_tags.join(","));
              }
            });
          }
        }
      });
      this.setup_awesomplete();
      this.setup_complete = true;
    }
    setup_awesomplete() {
      var me2 = this;
      var $input = this.wrapper.find("input.tags-input");
      var input = $input.get(0);
      this.awesomplete = new Awesomplete(input, {
        minChars: 0,
        maxItems: 99,
        list: []
      });
      $input.on("awesomplete-open", function(e) {
        $input.attr("state", "open");
      });
      $input.on("awesomplete-close", function(e) {
        $input.attr("state", "closed");
      });
      $input.on("input", function(e) {
        var value = e.target.value;
        frappe.call({
          method: "frappe.desk.doctype.tag.tag.get_tags",
          args: {
            doctype: me2.frm.doctype,
            txt: value.toLowerCase()
          },
          callback: function(r) {
            me2.awesomplete.list = r.message;
          }
        });
      });
      $input.on("focus", function(e) {
        if ($input.attr("state") != "open") {
          $input.trigger("input");
        }
      });
      $input.on("enter-pressed-in-addtag", function(e) {
        var value = e.target.value;
        if (value && value.trim()) {
          $input.trigger("input-selected");
          return;
        }
        frappe.call({
          method: "frappe.desk.doctype.tag.tag.get_tags",
          args: {
            doctype: me2.frm.doctype,
            txt: value.toLowerCase()
          },
          callback: function(r) {
            if (r.message.length)
              $input.val(r.message[0]);
            $input.trigger("input-selected");
          }
        });
      });
    }
    get_args(tag) {
      return {
        tag,
        dt: this.frm.doctype,
        dn: this.frm.docname
      };
    }
    refresh(user_tags) {
      var me2 = this;
      if (!this.initialized || !this.setup_complete || this.refreshing)
        return;
      me2.refreshing = true;
      try {
        me2.tags.clearTags();
        if (user_tags) {
          me2.user_tags = user_tags;
          me2.tags.addTags(user_tags.split(","));
        }
      } catch (e) {
        me2.refreshing = false;
        setTimeout(function() {
          me2.refresh();
        }, 100);
      }
      me2.refreshing = false;
    }
  };

  // node_modules/@popperjs/core/lib/enums.js
  var top = "top";
  var bottom = "bottom";
  var right = "right";
  var left = "left";
  var auto = "auto";
  var basePlacements = [top, bottom, right, left];
  var start = "start";
  var end = "end";
  var clippingParents = "clippingParents";
  var viewport = "viewport";
  var popper = "popper";
  var reference = "reference";
  var variationPlacements = /* @__PURE__ */ basePlacements.reduce(function(acc, placement) {
    return acc.concat([placement + "-" + start, placement + "-" + end]);
  }, []);
  var placements = /* @__PURE__ */ [].concat(basePlacements, [auto]).reduce(function(acc, placement) {
    return acc.concat([placement, placement + "-" + start, placement + "-" + end]);
  }, []);
  var beforeRead = "beforeRead";
  var read = "read";
  var afterRead = "afterRead";
  var beforeMain = "beforeMain";
  var main = "main";
  var afterMain = "afterMain";
  var beforeWrite = "beforeWrite";
  var write = "write";
  var afterWrite = "afterWrite";
  var modifierPhases = [beforeRead, read, afterRead, beforeMain, main, afterMain, beforeWrite, write, afterWrite];

  // node_modules/@popperjs/core/lib/dom-utils/getNodeName.js
  function getNodeName(element) {
    return element ? (element.nodeName || "").toLowerCase() : null;
  }

  // node_modules/@popperjs/core/lib/dom-utils/getWindow.js
  function getWindow(node) {
    if (node == null) {
      return window;
    }
    if (node.toString() !== "[object Window]") {
      var ownerDocument = node.ownerDocument;
      return ownerDocument ? ownerDocument.defaultView || window : window;
    }
    return node;
  }

  // node_modules/@popperjs/core/lib/dom-utils/instanceOf.js
  function isElement(node) {
    var OwnElement = getWindow(node).Element;
    return node instanceof OwnElement || node instanceof Element;
  }
  function isHTMLElement(node) {
    var OwnElement = getWindow(node).HTMLElement;
    return node instanceof OwnElement || node instanceof HTMLElement;
  }
  function isShadowRoot(node) {
    if (typeof ShadowRoot === "undefined") {
      return false;
    }
    var OwnElement = getWindow(node).ShadowRoot;
    return node instanceof OwnElement || node instanceof ShadowRoot;
  }

  // node_modules/@popperjs/core/lib/modifiers/applyStyles.js
  function applyStyles(_ref) {
    var state = _ref.state;
    Object.keys(state.elements).forEach(function(name) {
      var style = state.styles[name] || {};
      var attributes = state.attributes[name] || {};
      var element = state.elements[name];
      if (!isHTMLElement(element) || !getNodeName(element)) {
        return;
      }
      Object.assign(element.style, style);
      Object.keys(attributes).forEach(function(name2) {
        var value = attributes[name2];
        if (value === false) {
          element.removeAttribute(name2);
        } else {
          element.setAttribute(name2, value === true ? "" : value);
        }
      });
    });
  }
  function effect(_ref2) {
    var state = _ref2.state;
    var initialStyles = {
      popper: {
        position: state.options.strategy,
        left: "0",
        top: "0",
        margin: "0"
      },
      arrow: {
        position: "absolute"
      },
      reference: {}
    };
    Object.assign(state.elements.popper.style, initialStyles.popper);
    state.styles = initialStyles;
    if (state.elements.arrow) {
      Object.assign(state.elements.arrow.style, initialStyles.arrow);
    }
    return function() {
      Object.keys(state.elements).forEach(function(name) {
        var element = state.elements[name];
        var attributes = state.attributes[name] || {};
        var styleProperties = Object.keys(state.styles.hasOwnProperty(name) ? state.styles[name] : initialStyles[name]);
        var style = styleProperties.reduce(function(style2, property) {
          style2[property] = "";
          return style2;
        }, {});
        if (!isHTMLElement(element) || !getNodeName(element)) {
          return;
        }
        Object.assign(element.style, style);
        Object.keys(attributes).forEach(function(attribute) {
          element.removeAttribute(attribute);
        });
      });
    };
  }
  var applyStyles_default = {
    name: "applyStyles",
    enabled: true,
    phase: "write",
    fn: applyStyles,
    effect,
    requires: ["computeStyles"]
  };

  // node_modules/@popperjs/core/lib/utils/getBasePlacement.js
  function getBasePlacement(placement) {
    return placement.split("-")[0];
  }

  // node_modules/@popperjs/core/lib/utils/math.js
  var max = Math.max;
  var min = Math.min;
  var round = Math.round;

  // node_modules/@popperjs/core/lib/utils/userAgent.js
  function getUAString() {
    var uaData = navigator.userAgentData;
    if (uaData != null && uaData.brands && Array.isArray(uaData.brands)) {
      return uaData.brands.map(function(item) {
        return item.brand + "/" + item.version;
      }).join(" ");
    }
    return navigator.userAgent;
  }

  // node_modules/@popperjs/core/lib/dom-utils/isLayoutViewport.js
  function isLayoutViewport() {
    return !/^((?!chrome|android).)*safari/i.test(getUAString());
  }

  // node_modules/@popperjs/core/lib/dom-utils/getBoundingClientRect.js
  function getBoundingClientRect(element, includeScale, isFixedStrategy) {
    if (includeScale === void 0) {
      includeScale = false;
    }
    if (isFixedStrategy === void 0) {
      isFixedStrategy = false;
    }
    var clientRect = element.getBoundingClientRect();
    var scaleX = 1;
    var scaleY = 1;
    if (includeScale && isHTMLElement(element)) {
      scaleX = element.offsetWidth > 0 ? round(clientRect.width) / element.offsetWidth || 1 : 1;
      scaleY = element.offsetHeight > 0 ? round(clientRect.height) / element.offsetHeight || 1 : 1;
    }
    var _ref = isElement(element) ? getWindow(element) : window, visualViewport = _ref.visualViewport;
    var addVisualOffsets = !isLayoutViewport() && isFixedStrategy;
    var x = (clientRect.left + (addVisualOffsets && visualViewport ? visualViewport.offsetLeft : 0)) / scaleX;
    var y = (clientRect.top + (addVisualOffsets && visualViewport ? visualViewport.offsetTop : 0)) / scaleY;
    var width = clientRect.width / scaleX;
    var height = clientRect.height / scaleY;
    return {
      width,
      height,
      top: y,
      right: x + width,
      bottom: y + height,
      left: x,
      x,
      y
    };
  }

  // node_modules/@popperjs/core/lib/dom-utils/getLayoutRect.js
  function getLayoutRect(element) {
    var clientRect = getBoundingClientRect(element);
    var width = element.offsetWidth;
    var height = element.offsetHeight;
    if (Math.abs(clientRect.width - width) <= 1) {
      width = clientRect.width;
    }
    if (Math.abs(clientRect.height - height) <= 1) {
      height = clientRect.height;
    }
    return {
      x: element.offsetLeft,
      y: element.offsetTop,
      width,
      height
    };
  }

  // node_modules/@popperjs/core/lib/dom-utils/contains.js
  function contains(parent, child) {
    var rootNode = child.getRootNode && child.getRootNode();
    if (parent.contains(child)) {
      return true;
    } else if (rootNode && isShadowRoot(rootNode)) {
      var next = child;
      do {
        if (next && parent.isSameNode(next)) {
          return true;
        }
        next = next.parentNode || next.host;
      } while (next);
    }
    return false;
  }

  // node_modules/@popperjs/core/lib/dom-utils/getComputedStyle.js
  function getComputedStyle(element) {
    return getWindow(element).getComputedStyle(element);
  }

  // node_modules/@popperjs/core/lib/dom-utils/isTableElement.js
  function isTableElement(element) {
    return ["table", "td", "th"].indexOf(getNodeName(element)) >= 0;
  }

  // node_modules/@popperjs/core/lib/dom-utils/getDocumentElement.js
  function getDocumentElement(element) {
    return ((isElement(element) ? element.ownerDocument : element.document) || window.document).documentElement;
  }

  // node_modules/@popperjs/core/lib/dom-utils/getParentNode.js
  function getParentNode(element) {
    if (getNodeName(element) === "html") {
      return element;
    }
    return element.assignedSlot || element.parentNode || (isShadowRoot(element) ? element.host : null) || getDocumentElement(element);
  }

  // node_modules/@popperjs/core/lib/dom-utils/getOffsetParent.js
  function getTrueOffsetParent(element) {
    if (!isHTMLElement(element) || getComputedStyle(element).position === "fixed") {
      return null;
    }
    return element.offsetParent;
  }
  function getContainingBlock(element) {
    var isFirefox = /firefox/i.test(getUAString());
    var isIE = /Trident/i.test(getUAString());
    if (isIE && isHTMLElement(element)) {
      var elementCss = getComputedStyle(element);
      if (elementCss.position === "fixed") {
        return null;
      }
    }
    var currentNode = getParentNode(element);
    if (isShadowRoot(currentNode)) {
      currentNode = currentNode.host;
    }
    while (isHTMLElement(currentNode) && ["html", "body"].indexOf(getNodeName(currentNode)) < 0) {
      var css = getComputedStyle(currentNode);
      if (css.transform !== "none" || css.perspective !== "none" || css.contain === "paint" || ["transform", "perspective"].indexOf(css.willChange) !== -1 || isFirefox && css.willChange === "filter" || isFirefox && css.filter && css.filter !== "none") {
        return currentNode;
      } else {
        currentNode = currentNode.parentNode;
      }
    }
    return null;
  }
  function getOffsetParent(element) {
    var window2 = getWindow(element);
    var offsetParent = getTrueOffsetParent(element);
    while (offsetParent && isTableElement(offsetParent) && getComputedStyle(offsetParent).position === "static") {
      offsetParent = getTrueOffsetParent(offsetParent);
    }
    if (offsetParent && (getNodeName(offsetParent) === "html" || getNodeName(offsetParent) === "body" && getComputedStyle(offsetParent).position === "static")) {
      return window2;
    }
    return offsetParent || getContainingBlock(element) || window2;
  }

  // node_modules/@popperjs/core/lib/utils/getMainAxisFromPlacement.js
  function getMainAxisFromPlacement(placement) {
    return ["top", "bottom"].indexOf(placement) >= 0 ? "x" : "y";
  }

  // node_modules/@popperjs/core/lib/utils/within.js
  function within(min2, value, max2) {
    return max(min2, min(value, max2));
  }
  function withinMaxClamp(min2, value, max2) {
    var v = within(min2, value, max2);
    return v > max2 ? max2 : v;
  }

  // node_modules/@popperjs/core/lib/utils/getFreshSideObject.js
  function getFreshSideObject() {
    return {
      top: 0,
      right: 0,
      bottom: 0,
      left: 0
    };
  }

  // node_modules/@popperjs/core/lib/utils/mergePaddingObject.js
  function mergePaddingObject(paddingObject) {
    return Object.assign({}, getFreshSideObject(), paddingObject);
  }

  // node_modules/@popperjs/core/lib/utils/expandToHashMap.js
  function expandToHashMap(value, keys) {
    return keys.reduce(function(hashMap, key) {
      hashMap[key] = value;
      return hashMap;
    }, {});
  }

  // node_modules/@popperjs/core/lib/modifiers/arrow.js
  var toPaddingObject = function toPaddingObject2(padding, state) {
    padding = typeof padding === "function" ? padding(Object.assign({}, state.rects, {
      placement: state.placement
    })) : padding;
    return mergePaddingObject(typeof padding !== "number" ? padding : expandToHashMap(padding, basePlacements));
  };
  function arrow(_ref) {
    var _state$modifiersData$;
    var state = _ref.state, name = _ref.name, options = _ref.options;
    var arrowElement = state.elements.arrow;
    var popperOffsets2 = state.modifiersData.popperOffsets;
    var basePlacement = getBasePlacement(state.placement);
    var axis = getMainAxisFromPlacement(basePlacement);
    var isVertical = [left, right].indexOf(basePlacement) >= 0;
    var len = isVertical ? "height" : "width";
    if (!arrowElement || !popperOffsets2) {
      return;
    }
    var paddingObject = toPaddingObject(options.padding, state);
    var arrowRect = getLayoutRect(arrowElement);
    var minProp = axis === "y" ? top : left;
    var maxProp = axis === "y" ? bottom : right;
    var endDiff = state.rects.reference[len] + state.rects.reference[axis] - popperOffsets2[axis] - state.rects.popper[len];
    var startDiff = popperOffsets2[axis] - state.rects.reference[axis];
    var arrowOffsetParent = getOffsetParent(arrowElement);
    var clientSize = arrowOffsetParent ? axis === "y" ? arrowOffsetParent.clientHeight || 0 : arrowOffsetParent.clientWidth || 0 : 0;
    var centerToReference = endDiff / 2 - startDiff / 2;
    var min2 = paddingObject[minProp];
    var max2 = clientSize - arrowRect[len] - paddingObject[maxProp];
    var center = clientSize / 2 - arrowRect[len] / 2 + centerToReference;
    var offset2 = within(min2, center, max2);
    var axisProp = axis;
    state.modifiersData[name] = (_state$modifiersData$ = {}, _state$modifiersData$[axisProp] = offset2, _state$modifiersData$.centerOffset = offset2 - center, _state$modifiersData$);
  }
  function effect2(_ref2) {
    var state = _ref2.state, options = _ref2.options;
    var _options$element = options.element, arrowElement = _options$element === void 0 ? "[data-popper-arrow]" : _options$element;
    if (arrowElement == null) {
      return;
    }
    if (typeof arrowElement === "string") {
      arrowElement = state.elements.popper.querySelector(arrowElement);
      if (!arrowElement) {
        return;
      }
    }
    if (!contains(state.elements.popper, arrowElement)) {
      return;
    }
    state.elements.arrow = arrowElement;
  }
  var arrow_default = {
    name: "arrow",
    enabled: true,
    phase: "main",
    fn: arrow,
    effect: effect2,
    requires: ["popperOffsets"],
    requiresIfExists: ["preventOverflow"]
  };

  // node_modules/@popperjs/core/lib/utils/getVariation.js
  function getVariation(placement) {
    return placement.split("-")[1];
  }

  // node_modules/@popperjs/core/lib/modifiers/computeStyles.js
  var unsetSides = {
    top: "auto",
    right: "auto",
    bottom: "auto",
    left: "auto"
  };
  function roundOffsetsByDPR(_ref, win) {
    var x = _ref.x, y = _ref.y;
    var dpr = win.devicePixelRatio || 1;
    return {
      x: round(x * dpr) / dpr || 0,
      y: round(y * dpr) / dpr || 0
    };
  }
  function mapToStyles(_ref2) {
    var _Object$assign2;
    var popper2 = _ref2.popper, popperRect = _ref2.popperRect, placement = _ref2.placement, variation = _ref2.variation, offsets = _ref2.offsets, position = _ref2.position, gpuAcceleration = _ref2.gpuAcceleration, adaptive = _ref2.adaptive, roundOffsets = _ref2.roundOffsets, isFixed = _ref2.isFixed;
    var _offsets$x = offsets.x, x = _offsets$x === void 0 ? 0 : _offsets$x, _offsets$y = offsets.y, y = _offsets$y === void 0 ? 0 : _offsets$y;
    var _ref3 = typeof roundOffsets === "function" ? roundOffsets({
      x,
      y
    }) : {
      x,
      y
    };
    x = _ref3.x;
    y = _ref3.y;
    var hasX = offsets.hasOwnProperty("x");
    var hasY = offsets.hasOwnProperty("y");
    var sideX = left;
    var sideY = top;
    var win = window;
    if (adaptive) {
      var offsetParent = getOffsetParent(popper2);
      var heightProp = "clientHeight";
      var widthProp = "clientWidth";
      if (offsetParent === getWindow(popper2)) {
        offsetParent = getDocumentElement(popper2);
        if (getComputedStyle(offsetParent).position !== "static" && position === "absolute") {
          heightProp = "scrollHeight";
          widthProp = "scrollWidth";
        }
      }
      offsetParent = offsetParent;
      if (placement === top || (placement === left || placement === right) && variation === end) {
        sideY = bottom;
        var offsetY = isFixed && offsetParent === win && win.visualViewport ? win.visualViewport.height : offsetParent[heightProp];
        y -= offsetY - popperRect.height;
        y *= gpuAcceleration ? 1 : -1;
      }
      if (placement === left || (placement === top || placement === bottom) && variation === end) {
        sideX = right;
        var offsetX = isFixed && offsetParent === win && win.visualViewport ? win.visualViewport.width : offsetParent[widthProp];
        x -= offsetX - popperRect.width;
        x *= gpuAcceleration ? 1 : -1;
      }
    }
    var commonStyles = Object.assign({
      position
    }, adaptive && unsetSides);
    var _ref4 = roundOffsets === true ? roundOffsetsByDPR({
      x,
      y
    }, getWindow(popper2)) : {
      x,
      y
    };
    x = _ref4.x;
    y = _ref4.y;
    if (gpuAcceleration) {
      var _Object$assign;
      return Object.assign({}, commonStyles, (_Object$assign = {}, _Object$assign[sideY] = hasY ? "0" : "", _Object$assign[sideX] = hasX ? "0" : "", _Object$assign.transform = (win.devicePixelRatio || 1) <= 1 ? "translate(" + x + "px, " + y + "px)" : "translate3d(" + x + "px, " + y + "px, 0)", _Object$assign));
    }
    return Object.assign({}, commonStyles, (_Object$assign2 = {}, _Object$assign2[sideY] = hasY ? y + "px" : "", _Object$assign2[sideX] = hasX ? x + "px" : "", _Object$assign2.transform = "", _Object$assign2));
  }
  function computeStyles(_ref5) {
    var state = _ref5.state, options = _ref5.options;
    var _options$gpuAccelerat = options.gpuAcceleration, gpuAcceleration = _options$gpuAccelerat === void 0 ? true : _options$gpuAccelerat, _options$adaptive = options.adaptive, adaptive = _options$adaptive === void 0 ? true : _options$adaptive, _options$roundOffsets = options.roundOffsets, roundOffsets = _options$roundOffsets === void 0 ? true : _options$roundOffsets;
    var commonStyles = {
      placement: getBasePlacement(state.placement),
      variation: getVariation(state.placement),
      popper: state.elements.popper,
      popperRect: state.rects.popper,
      gpuAcceleration,
      isFixed: state.options.strategy === "fixed"
    };
    if (state.modifiersData.popperOffsets != null) {
      state.styles.popper = Object.assign({}, state.styles.popper, mapToStyles(Object.assign({}, commonStyles, {
        offsets: state.modifiersData.popperOffsets,
        position: state.options.strategy,
        adaptive,
        roundOffsets
      })));
    }
    if (state.modifiersData.arrow != null) {
      state.styles.arrow = Object.assign({}, state.styles.arrow, mapToStyles(Object.assign({}, commonStyles, {
        offsets: state.modifiersData.arrow,
        position: "absolute",
        adaptive: false,
        roundOffsets
      })));
    }
    state.attributes.popper = Object.assign({}, state.attributes.popper, {
      "data-popper-placement": state.placement
    });
  }
  var computeStyles_default = {
    name: "computeStyles",
    enabled: true,
    phase: "beforeWrite",
    fn: computeStyles,
    data: {}
  };

  // node_modules/@popperjs/core/lib/modifiers/eventListeners.js
  var passive = {
    passive: true
  };
  function effect3(_ref) {
    var state = _ref.state, instance = _ref.instance, options = _ref.options;
    var _options$scroll = options.scroll, scroll = _options$scroll === void 0 ? true : _options$scroll, _options$resize = options.resize, resize = _options$resize === void 0 ? true : _options$resize;
    var window2 = getWindow(state.elements.popper);
    var scrollParents = [].concat(state.scrollParents.reference, state.scrollParents.popper);
    if (scroll) {
      scrollParents.forEach(function(scrollParent) {
        scrollParent.addEventListener("scroll", instance.update, passive);
      });
    }
    if (resize) {
      window2.addEventListener("resize", instance.update, passive);
    }
    return function() {
      if (scroll) {
        scrollParents.forEach(function(scrollParent) {
          scrollParent.removeEventListener("scroll", instance.update, passive);
        });
      }
      if (resize) {
        window2.removeEventListener("resize", instance.update, passive);
      }
    };
  }
  var eventListeners_default = {
    name: "eventListeners",
    enabled: true,
    phase: "write",
    fn: function fn() {
    },
    effect: effect3,
    data: {}
  };

  // node_modules/@popperjs/core/lib/utils/getOppositePlacement.js
  var hash = {
    left: "right",
    right: "left",
    bottom: "top",
    top: "bottom"
  };
  function getOppositePlacement(placement) {
    return placement.replace(/left|right|bottom|top/g, function(matched) {
      return hash[matched];
    });
  }

  // node_modules/@popperjs/core/lib/utils/getOppositeVariationPlacement.js
  var hash2 = {
    start: "end",
    end: "start"
  };
  function getOppositeVariationPlacement(placement) {
    return placement.replace(/start|end/g, function(matched) {
      return hash2[matched];
    });
  }

  // node_modules/@popperjs/core/lib/dom-utils/getWindowScroll.js
  function getWindowScroll(node) {
    var win = getWindow(node);
    var scrollLeft = win.pageXOffset;
    var scrollTop = win.pageYOffset;
    return {
      scrollLeft,
      scrollTop
    };
  }

  // node_modules/@popperjs/core/lib/dom-utils/getWindowScrollBarX.js
  function getWindowScrollBarX(element) {
    return getBoundingClientRect(getDocumentElement(element)).left + getWindowScroll(element).scrollLeft;
  }

  // node_modules/@popperjs/core/lib/dom-utils/getViewportRect.js
  function getViewportRect(element, strategy) {
    var win = getWindow(element);
    var html = getDocumentElement(element);
    var visualViewport = win.visualViewport;
    var width = html.clientWidth;
    var height = html.clientHeight;
    var x = 0;
    var y = 0;
    if (visualViewport) {
      width = visualViewport.width;
      height = visualViewport.height;
      var layoutViewport = isLayoutViewport();
      if (layoutViewport || !layoutViewport && strategy === "fixed") {
        x = visualViewport.offsetLeft;
        y = visualViewport.offsetTop;
      }
    }
    return {
      width,
      height,
      x: x + getWindowScrollBarX(element),
      y
    };
  }

  // node_modules/@popperjs/core/lib/dom-utils/getDocumentRect.js
  function getDocumentRect(element) {
    var _element$ownerDocumen;
    var html = getDocumentElement(element);
    var winScroll = getWindowScroll(element);
    var body = (_element$ownerDocumen = element.ownerDocument) == null ? void 0 : _element$ownerDocumen.body;
    var width = max(html.scrollWidth, html.clientWidth, body ? body.scrollWidth : 0, body ? body.clientWidth : 0);
    var height = max(html.scrollHeight, html.clientHeight, body ? body.scrollHeight : 0, body ? body.clientHeight : 0);
    var x = -winScroll.scrollLeft + getWindowScrollBarX(element);
    var y = -winScroll.scrollTop;
    if (getComputedStyle(body || html).direction === "rtl") {
      x += max(html.clientWidth, body ? body.clientWidth : 0) - width;
    }
    return {
      width,
      height,
      x,
      y
    };
  }

  // node_modules/@popperjs/core/lib/dom-utils/isScrollParent.js
  function isScrollParent(element) {
    var _getComputedStyle = getComputedStyle(element), overflow = _getComputedStyle.overflow, overflowX = _getComputedStyle.overflowX, overflowY = _getComputedStyle.overflowY;
    return /auto|scroll|overlay|hidden/.test(overflow + overflowY + overflowX);
  }

  // node_modules/@popperjs/core/lib/dom-utils/getScrollParent.js
  function getScrollParent(node) {
    if (["html", "body", "#document"].indexOf(getNodeName(node)) >= 0) {
      return node.ownerDocument.body;
    }
    if (isHTMLElement(node) && isScrollParent(node)) {
      return node;
    }
    return getScrollParent(getParentNode(node));
  }

  // node_modules/@popperjs/core/lib/dom-utils/listScrollParents.js
  function listScrollParents(element, list) {
    var _element$ownerDocumen;
    if (list === void 0) {
      list = [];
    }
    var scrollParent = getScrollParent(element);
    var isBody = scrollParent === ((_element$ownerDocumen = element.ownerDocument) == null ? void 0 : _element$ownerDocumen.body);
    var win = getWindow(scrollParent);
    var target = isBody ? [win].concat(win.visualViewport || [], isScrollParent(scrollParent) ? scrollParent : []) : scrollParent;
    var updatedList = list.concat(target);
    return isBody ? updatedList : updatedList.concat(listScrollParents(getParentNode(target)));
  }

  // node_modules/@popperjs/core/lib/utils/rectToClientRect.js
  function rectToClientRect(rect) {
    return Object.assign({}, rect, {
      left: rect.x,
      top: rect.y,
      right: rect.x + rect.width,
      bottom: rect.y + rect.height
    });
  }

  // node_modules/@popperjs/core/lib/dom-utils/getClippingRect.js
  function getInnerBoundingClientRect(element, strategy) {
    var rect = getBoundingClientRect(element, false, strategy === "fixed");
    rect.top = rect.top + element.clientTop;
    rect.left = rect.left + element.clientLeft;
    rect.bottom = rect.top + element.clientHeight;
    rect.right = rect.left + element.clientWidth;
    rect.width = element.clientWidth;
    rect.height = element.clientHeight;
    rect.x = rect.left;
    rect.y = rect.top;
    return rect;
  }
  function getClientRectFromMixedType(element, clippingParent, strategy) {
    return clippingParent === viewport ? rectToClientRect(getViewportRect(element, strategy)) : isElement(clippingParent) ? getInnerBoundingClientRect(clippingParent, strategy) : rectToClientRect(getDocumentRect(getDocumentElement(element)));
  }
  function getClippingParents(element) {
    var clippingParents2 = listScrollParents(getParentNode(element));
    var canEscapeClipping = ["absolute", "fixed"].indexOf(getComputedStyle(element).position) >= 0;
    var clipperElement = canEscapeClipping && isHTMLElement(element) ? getOffsetParent(element) : element;
    if (!isElement(clipperElement)) {
      return [];
    }
    return clippingParents2.filter(function(clippingParent) {
      return isElement(clippingParent) && contains(clippingParent, clipperElement) && getNodeName(clippingParent) !== "body";
    });
  }
  function getClippingRect(element, boundary, rootBoundary, strategy) {
    var mainClippingParents = boundary === "clippingParents" ? getClippingParents(element) : [].concat(boundary);
    var clippingParents2 = [].concat(mainClippingParents, [rootBoundary]);
    var firstClippingParent = clippingParents2[0];
    var clippingRect = clippingParents2.reduce(function(accRect, clippingParent) {
      var rect = getClientRectFromMixedType(element, clippingParent, strategy);
      accRect.top = max(rect.top, accRect.top);
      accRect.right = min(rect.right, accRect.right);
      accRect.bottom = min(rect.bottom, accRect.bottom);
      accRect.left = max(rect.left, accRect.left);
      return accRect;
    }, getClientRectFromMixedType(element, firstClippingParent, strategy));
    clippingRect.width = clippingRect.right - clippingRect.left;
    clippingRect.height = clippingRect.bottom - clippingRect.top;
    clippingRect.x = clippingRect.left;
    clippingRect.y = clippingRect.top;
    return clippingRect;
  }

  // node_modules/@popperjs/core/lib/utils/computeOffsets.js
  function computeOffsets(_ref) {
    var reference2 = _ref.reference, element = _ref.element, placement = _ref.placement;
    var basePlacement = placement ? getBasePlacement(placement) : null;
    var variation = placement ? getVariation(placement) : null;
    var commonX = reference2.x + reference2.width / 2 - element.width / 2;
    var commonY = reference2.y + reference2.height / 2 - element.height / 2;
    var offsets;
    switch (basePlacement) {
      case top:
        offsets = {
          x: commonX,
          y: reference2.y - element.height
        };
        break;
      case bottom:
        offsets = {
          x: commonX,
          y: reference2.y + reference2.height
        };
        break;
      case right:
        offsets = {
          x: reference2.x + reference2.width,
          y: commonY
        };
        break;
      case left:
        offsets = {
          x: reference2.x - element.width,
          y: commonY
        };
        break;
      default:
        offsets = {
          x: reference2.x,
          y: reference2.y
        };
    }
    var mainAxis = basePlacement ? getMainAxisFromPlacement(basePlacement) : null;
    if (mainAxis != null) {
      var len = mainAxis === "y" ? "height" : "width";
      switch (variation) {
        case start:
          offsets[mainAxis] = offsets[mainAxis] - (reference2[len] / 2 - element[len] / 2);
          break;
        case end:
          offsets[mainAxis] = offsets[mainAxis] + (reference2[len] / 2 - element[len] / 2);
          break;
        default:
      }
    }
    return offsets;
  }

  // node_modules/@popperjs/core/lib/utils/detectOverflow.js
  function detectOverflow(state, options) {
    if (options === void 0) {
      options = {};
    }
    var _options = options, _options$placement = _options.placement, placement = _options$placement === void 0 ? state.placement : _options$placement, _options$strategy = _options.strategy, strategy = _options$strategy === void 0 ? state.strategy : _options$strategy, _options$boundary = _options.boundary, boundary = _options$boundary === void 0 ? clippingParents : _options$boundary, _options$rootBoundary = _options.rootBoundary, rootBoundary = _options$rootBoundary === void 0 ? viewport : _options$rootBoundary, _options$elementConte = _options.elementContext, elementContext = _options$elementConte === void 0 ? popper : _options$elementConte, _options$altBoundary = _options.altBoundary, altBoundary = _options$altBoundary === void 0 ? false : _options$altBoundary, _options$padding = _options.padding, padding = _options$padding === void 0 ? 0 : _options$padding;
    var paddingObject = mergePaddingObject(typeof padding !== "number" ? padding : expandToHashMap(padding, basePlacements));
    var altContext = elementContext === popper ? reference : popper;
    var popperRect = state.rects.popper;
    var element = state.elements[altBoundary ? altContext : elementContext];
    var clippingClientRect = getClippingRect(isElement(element) ? element : element.contextElement || getDocumentElement(state.elements.popper), boundary, rootBoundary, strategy);
    var referenceClientRect = getBoundingClientRect(state.elements.reference);
    var popperOffsets2 = computeOffsets({
      reference: referenceClientRect,
      element: popperRect,
      strategy: "absolute",
      placement
    });
    var popperClientRect = rectToClientRect(Object.assign({}, popperRect, popperOffsets2));
    var elementClientRect = elementContext === popper ? popperClientRect : referenceClientRect;
    var overflowOffsets = {
      top: clippingClientRect.top - elementClientRect.top + paddingObject.top,
      bottom: elementClientRect.bottom - clippingClientRect.bottom + paddingObject.bottom,
      left: clippingClientRect.left - elementClientRect.left + paddingObject.left,
      right: elementClientRect.right - clippingClientRect.right + paddingObject.right
    };
    var offsetData = state.modifiersData.offset;
    if (elementContext === popper && offsetData) {
      var offset2 = offsetData[placement];
      Object.keys(overflowOffsets).forEach(function(key) {
        var multiply = [right, bottom].indexOf(key) >= 0 ? 1 : -1;
        var axis = [top, bottom].indexOf(key) >= 0 ? "y" : "x";
        overflowOffsets[key] += offset2[axis] * multiply;
      });
    }
    return overflowOffsets;
  }

  // node_modules/@popperjs/core/lib/utils/computeAutoPlacement.js
  function computeAutoPlacement(state, options) {
    if (options === void 0) {
      options = {};
    }
    var _options = options, placement = _options.placement, boundary = _options.boundary, rootBoundary = _options.rootBoundary, padding = _options.padding, flipVariations = _options.flipVariations, _options$allowedAutoP = _options.allowedAutoPlacements, allowedAutoPlacements = _options$allowedAutoP === void 0 ? placements : _options$allowedAutoP;
    var variation = getVariation(placement);
    var placements2 = variation ? flipVariations ? variationPlacements : variationPlacements.filter(function(placement2) {
      return getVariation(placement2) === variation;
    }) : basePlacements;
    var allowedPlacements = placements2.filter(function(placement2) {
      return allowedAutoPlacements.indexOf(placement2) >= 0;
    });
    if (allowedPlacements.length === 0) {
      allowedPlacements = placements2;
    }
    var overflows = allowedPlacements.reduce(function(acc, placement2) {
      acc[placement2] = detectOverflow(state, {
        placement: placement2,
        boundary,
        rootBoundary,
        padding
      })[getBasePlacement(placement2)];
      return acc;
    }, {});
    return Object.keys(overflows).sort(function(a, b) {
      return overflows[a] - overflows[b];
    });
  }

  // node_modules/@popperjs/core/lib/modifiers/flip.js
  function getExpandedFallbackPlacements(placement) {
    if (getBasePlacement(placement) === auto) {
      return [];
    }
    var oppositePlacement = getOppositePlacement(placement);
    return [getOppositeVariationPlacement(placement), oppositePlacement, getOppositeVariationPlacement(oppositePlacement)];
  }
  function flip(_ref) {
    var state = _ref.state, options = _ref.options, name = _ref.name;
    if (state.modifiersData[name]._skip) {
      return;
    }
    var _options$mainAxis = options.mainAxis, checkMainAxis = _options$mainAxis === void 0 ? true : _options$mainAxis, _options$altAxis = options.altAxis, checkAltAxis = _options$altAxis === void 0 ? true : _options$altAxis, specifiedFallbackPlacements = options.fallbackPlacements, padding = options.padding, boundary = options.boundary, rootBoundary = options.rootBoundary, altBoundary = options.altBoundary, _options$flipVariatio = options.flipVariations, flipVariations = _options$flipVariatio === void 0 ? true : _options$flipVariatio, allowedAutoPlacements = options.allowedAutoPlacements;
    var preferredPlacement = state.options.placement;
    var basePlacement = getBasePlacement(preferredPlacement);
    var isBasePlacement = basePlacement === preferredPlacement;
    var fallbackPlacements = specifiedFallbackPlacements || (isBasePlacement || !flipVariations ? [getOppositePlacement(preferredPlacement)] : getExpandedFallbackPlacements(preferredPlacement));
    var placements2 = [preferredPlacement].concat(fallbackPlacements).reduce(function(acc, placement2) {
      return acc.concat(getBasePlacement(placement2) === auto ? computeAutoPlacement(state, {
        placement: placement2,
        boundary,
        rootBoundary,
        padding,
        flipVariations,
        allowedAutoPlacements
      }) : placement2);
    }, []);
    var referenceRect = state.rects.reference;
    var popperRect = state.rects.popper;
    var checksMap = /* @__PURE__ */ new Map();
    var makeFallbackChecks = true;
    var firstFittingPlacement = placements2[0];
    for (var i2 = 0; i2 < placements2.length; i2++) {
      var placement = placements2[i2];
      var _basePlacement = getBasePlacement(placement);
      var isStartVariation = getVariation(placement) === start;
      var isVertical = [top, bottom].indexOf(_basePlacement) >= 0;
      var len = isVertical ? "width" : "height";
      var overflow = detectOverflow(state, {
        placement,
        boundary,
        rootBoundary,
        altBoundary,
        padding
      });
      var mainVariationSide = isVertical ? isStartVariation ? right : left : isStartVariation ? bottom : top;
      if (referenceRect[len] > popperRect[len]) {
        mainVariationSide = getOppositePlacement(mainVariationSide);
      }
      var altVariationSide = getOppositePlacement(mainVariationSide);
      var checks = [];
      if (checkMainAxis) {
        checks.push(overflow[_basePlacement] <= 0);
      }
      if (checkAltAxis) {
        checks.push(overflow[mainVariationSide] <= 0, overflow[altVariationSide] <= 0);
      }
      if (checks.every(function(check) {
        return check;
      })) {
        firstFittingPlacement = placement;
        makeFallbackChecks = false;
        break;
      }
      checksMap.set(placement, checks);
    }
    if (makeFallbackChecks) {
      var numberOfChecks = flipVariations ? 3 : 1;
      var _loop = function _loop2(_i2) {
        var fittingPlacement = placements2.find(function(placement2) {
          var checks2 = checksMap.get(placement2);
          if (checks2) {
            return checks2.slice(0, _i2).every(function(check) {
              return check;
            });
          }
        });
        if (fittingPlacement) {
          firstFittingPlacement = fittingPlacement;
          return "break";
        }
      };
      for (var _i = numberOfChecks; _i > 0; _i--) {
        var _ret = _loop(_i);
        if (_ret === "break")
          break;
      }
    }
    if (state.placement !== firstFittingPlacement) {
      state.modifiersData[name]._skip = true;
      state.placement = firstFittingPlacement;
      state.reset = true;
    }
  }
  var flip_default = {
    name: "flip",
    enabled: true,
    phase: "main",
    fn: flip,
    requiresIfExists: ["offset"],
    data: {
      _skip: false
    }
  };

  // node_modules/@popperjs/core/lib/modifiers/hide.js
  function getSideOffsets(overflow, rect, preventedOffsets) {
    if (preventedOffsets === void 0) {
      preventedOffsets = {
        x: 0,
        y: 0
      };
    }
    return {
      top: overflow.top - rect.height - preventedOffsets.y,
      right: overflow.right - rect.width + preventedOffsets.x,
      bottom: overflow.bottom - rect.height + preventedOffsets.y,
      left: overflow.left - rect.width - preventedOffsets.x
    };
  }
  function isAnySideFullyClipped(overflow) {
    return [top, right, bottom, left].some(function(side) {
      return overflow[side] >= 0;
    });
  }
  function hide(_ref) {
    var state = _ref.state, name = _ref.name;
    var referenceRect = state.rects.reference;
    var popperRect = state.rects.popper;
    var preventedOffsets = state.modifiersData.preventOverflow;
    var referenceOverflow = detectOverflow(state, {
      elementContext: "reference"
    });
    var popperAltOverflow = detectOverflow(state, {
      altBoundary: true
    });
    var referenceClippingOffsets = getSideOffsets(referenceOverflow, referenceRect);
    var popperEscapeOffsets = getSideOffsets(popperAltOverflow, popperRect, preventedOffsets);
    var isReferenceHidden = isAnySideFullyClipped(referenceClippingOffsets);
    var hasPopperEscaped = isAnySideFullyClipped(popperEscapeOffsets);
    state.modifiersData[name] = {
      referenceClippingOffsets,
      popperEscapeOffsets,
      isReferenceHidden,
      hasPopperEscaped
    };
    state.attributes.popper = Object.assign({}, state.attributes.popper, {
      "data-popper-reference-hidden": isReferenceHidden,
      "data-popper-escaped": hasPopperEscaped
    });
  }
  var hide_default = {
    name: "hide",
    enabled: true,
    phase: "main",
    requiresIfExists: ["preventOverflow"],
    fn: hide
  };

  // node_modules/@popperjs/core/lib/modifiers/offset.js
  function distanceAndSkiddingToXY(placement, rects, offset2) {
    var basePlacement = getBasePlacement(placement);
    var invertDistance = [left, top].indexOf(basePlacement) >= 0 ? -1 : 1;
    var _ref = typeof offset2 === "function" ? offset2(Object.assign({}, rects, {
      placement
    })) : offset2, skidding = _ref[0], distance = _ref[1];
    skidding = skidding || 0;
    distance = (distance || 0) * invertDistance;
    return [left, right].indexOf(basePlacement) >= 0 ? {
      x: distance,
      y: skidding
    } : {
      x: skidding,
      y: distance
    };
  }
  function offset(_ref2) {
    var state = _ref2.state, options = _ref2.options, name = _ref2.name;
    var _options$offset = options.offset, offset2 = _options$offset === void 0 ? [0, 0] : _options$offset;
    var data = placements.reduce(function(acc, placement) {
      acc[placement] = distanceAndSkiddingToXY(placement, state.rects, offset2);
      return acc;
    }, {});
    var _data$state$placement = data[state.placement], x = _data$state$placement.x, y = _data$state$placement.y;
    if (state.modifiersData.popperOffsets != null) {
      state.modifiersData.popperOffsets.x += x;
      state.modifiersData.popperOffsets.y += y;
    }
    state.modifiersData[name] = data;
  }
  var offset_default = {
    name: "offset",
    enabled: true,
    phase: "main",
    requires: ["popperOffsets"],
    fn: offset
  };

  // node_modules/@popperjs/core/lib/modifiers/popperOffsets.js
  function popperOffsets(_ref) {
    var state = _ref.state, name = _ref.name;
    state.modifiersData[name] = computeOffsets({
      reference: state.rects.reference,
      element: state.rects.popper,
      strategy: "absolute",
      placement: state.placement
    });
  }
  var popperOffsets_default = {
    name: "popperOffsets",
    enabled: true,
    phase: "read",
    fn: popperOffsets,
    data: {}
  };

  // node_modules/@popperjs/core/lib/utils/getAltAxis.js
  function getAltAxis(axis) {
    return axis === "x" ? "y" : "x";
  }

  // node_modules/@popperjs/core/lib/modifiers/preventOverflow.js
  function preventOverflow(_ref) {
    var state = _ref.state, options = _ref.options, name = _ref.name;
    var _options$mainAxis = options.mainAxis, checkMainAxis = _options$mainAxis === void 0 ? true : _options$mainAxis, _options$altAxis = options.altAxis, checkAltAxis = _options$altAxis === void 0 ? false : _options$altAxis, boundary = options.boundary, rootBoundary = options.rootBoundary, altBoundary = options.altBoundary, padding = options.padding, _options$tether = options.tether, tether = _options$tether === void 0 ? true : _options$tether, _options$tetherOffset = options.tetherOffset, tetherOffset = _options$tetherOffset === void 0 ? 0 : _options$tetherOffset;
    var overflow = detectOverflow(state, {
      boundary,
      rootBoundary,
      padding,
      altBoundary
    });
    var basePlacement = getBasePlacement(state.placement);
    var variation = getVariation(state.placement);
    var isBasePlacement = !variation;
    var mainAxis = getMainAxisFromPlacement(basePlacement);
    var altAxis = getAltAxis(mainAxis);
    var popperOffsets2 = state.modifiersData.popperOffsets;
    var referenceRect = state.rects.reference;
    var popperRect = state.rects.popper;
    var tetherOffsetValue = typeof tetherOffset === "function" ? tetherOffset(Object.assign({}, state.rects, {
      placement: state.placement
    })) : tetherOffset;
    var normalizedTetherOffsetValue = typeof tetherOffsetValue === "number" ? {
      mainAxis: tetherOffsetValue,
      altAxis: tetherOffsetValue
    } : Object.assign({
      mainAxis: 0,
      altAxis: 0
    }, tetherOffsetValue);
    var offsetModifierState = state.modifiersData.offset ? state.modifiersData.offset[state.placement] : null;
    var data = {
      x: 0,
      y: 0
    };
    if (!popperOffsets2) {
      return;
    }
    if (checkMainAxis) {
      var _offsetModifierState$;
      var mainSide = mainAxis === "y" ? top : left;
      var altSide = mainAxis === "y" ? bottom : right;
      var len = mainAxis === "y" ? "height" : "width";
      var offset2 = popperOffsets2[mainAxis];
      var min2 = offset2 + overflow[mainSide];
      var max2 = offset2 - overflow[altSide];
      var additive = tether ? -popperRect[len] / 2 : 0;
      var minLen = variation === start ? referenceRect[len] : popperRect[len];
      var maxLen = variation === start ? -popperRect[len] : -referenceRect[len];
      var arrowElement = state.elements.arrow;
      var arrowRect = tether && arrowElement ? getLayoutRect(arrowElement) : {
        width: 0,
        height: 0
      };
      var arrowPaddingObject = state.modifiersData["arrow#persistent"] ? state.modifiersData["arrow#persistent"].padding : getFreshSideObject();
      var arrowPaddingMin = arrowPaddingObject[mainSide];
      var arrowPaddingMax = arrowPaddingObject[altSide];
      var arrowLen = within(0, referenceRect[len], arrowRect[len]);
      var minOffset = isBasePlacement ? referenceRect[len] / 2 - additive - arrowLen - arrowPaddingMin - normalizedTetherOffsetValue.mainAxis : minLen - arrowLen - arrowPaddingMin - normalizedTetherOffsetValue.mainAxis;
      var maxOffset = isBasePlacement ? -referenceRect[len] / 2 + additive + arrowLen + arrowPaddingMax + normalizedTetherOffsetValue.mainAxis : maxLen + arrowLen + arrowPaddingMax + normalizedTetherOffsetValue.mainAxis;
      var arrowOffsetParent = state.elements.arrow && getOffsetParent(state.elements.arrow);
      var clientOffset = arrowOffsetParent ? mainAxis === "y" ? arrowOffsetParent.clientTop || 0 : arrowOffsetParent.clientLeft || 0 : 0;
      var offsetModifierValue = (_offsetModifierState$ = offsetModifierState == null ? void 0 : offsetModifierState[mainAxis]) != null ? _offsetModifierState$ : 0;
      var tetherMin = offset2 + minOffset - offsetModifierValue - clientOffset;
      var tetherMax = offset2 + maxOffset - offsetModifierValue;
      var preventedOffset = within(tether ? min(min2, tetherMin) : min2, offset2, tether ? max(max2, tetherMax) : max2);
      popperOffsets2[mainAxis] = preventedOffset;
      data[mainAxis] = preventedOffset - offset2;
    }
    if (checkAltAxis) {
      var _offsetModifierState$2;
      var _mainSide = mainAxis === "x" ? top : left;
      var _altSide = mainAxis === "x" ? bottom : right;
      var _offset = popperOffsets2[altAxis];
      var _len = altAxis === "y" ? "height" : "width";
      var _min = _offset + overflow[_mainSide];
      var _max = _offset - overflow[_altSide];
      var isOriginSide = [top, left].indexOf(basePlacement) !== -1;
      var _offsetModifierValue = (_offsetModifierState$2 = offsetModifierState == null ? void 0 : offsetModifierState[altAxis]) != null ? _offsetModifierState$2 : 0;
      var _tetherMin = isOriginSide ? _min : _offset - referenceRect[_len] - popperRect[_len] - _offsetModifierValue + normalizedTetherOffsetValue.altAxis;
      var _tetherMax = isOriginSide ? _offset + referenceRect[_len] + popperRect[_len] - _offsetModifierValue - normalizedTetherOffsetValue.altAxis : _max;
      var _preventedOffset = tether && isOriginSide ? withinMaxClamp(_tetherMin, _offset, _tetherMax) : within(tether ? _tetherMin : _min, _offset, tether ? _tetherMax : _max);
      popperOffsets2[altAxis] = _preventedOffset;
      data[altAxis] = _preventedOffset - _offset;
    }
    state.modifiersData[name] = data;
  }
  var preventOverflow_default = {
    name: "preventOverflow",
    enabled: true,
    phase: "main",
    fn: preventOverflow,
    requiresIfExists: ["offset"]
  };

  // node_modules/@popperjs/core/lib/dom-utils/getHTMLElementScroll.js
  function getHTMLElementScroll(element) {
    return {
      scrollLeft: element.scrollLeft,
      scrollTop: element.scrollTop
    };
  }

  // node_modules/@popperjs/core/lib/dom-utils/getNodeScroll.js
  function getNodeScroll(node) {
    if (node === getWindow(node) || !isHTMLElement(node)) {
      return getWindowScroll(node);
    } else {
      return getHTMLElementScroll(node);
    }
  }

  // node_modules/@popperjs/core/lib/dom-utils/getCompositeRect.js
  function isElementScaled(element) {
    var rect = element.getBoundingClientRect();
    var scaleX = round(rect.width) / element.offsetWidth || 1;
    var scaleY = round(rect.height) / element.offsetHeight || 1;
    return scaleX !== 1 || scaleY !== 1;
  }
  function getCompositeRect(elementOrVirtualElement, offsetParent, isFixed) {
    if (isFixed === void 0) {
      isFixed = false;
    }
    var isOffsetParentAnElement = isHTMLElement(offsetParent);
    var offsetParentIsScaled = isHTMLElement(offsetParent) && isElementScaled(offsetParent);
    var documentElement = getDocumentElement(offsetParent);
    var rect = getBoundingClientRect(elementOrVirtualElement, offsetParentIsScaled, isFixed);
    var scroll = {
      scrollLeft: 0,
      scrollTop: 0
    };
    var offsets = {
      x: 0,
      y: 0
    };
    if (isOffsetParentAnElement || !isOffsetParentAnElement && !isFixed) {
      if (getNodeName(offsetParent) !== "body" || isScrollParent(documentElement)) {
        scroll = getNodeScroll(offsetParent);
      }
      if (isHTMLElement(offsetParent)) {
        offsets = getBoundingClientRect(offsetParent, true);
        offsets.x += offsetParent.clientLeft;
        offsets.y += offsetParent.clientTop;
      } else if (documentElement) {
        offsets.x = getWindowScrollBarX(documentElement);
      }
    }
    return {
      x: rect.left + scroll.scrollLeft - offsets.x,
      y: rect.top + scroll.scrollTop - offsets.y,
      width: rect.width,
      height: rect.height
    };
  }

  // node_modules/@popperjs/core/lib/utils/orderModifiers.js
  function order(modifiers) {
    var map = /* @__PURE__ */ new Map();
    var visited = /* @__PURE__ */ new Set();
    var result = [];
    modifiers.forEach(function(modifier) {
      map.set(modifier.name, modifier);
    });
    function sort(modifier) {
      visited.add(modifier.name);
      var requires = [].concat(modifier.requires || [], modifier.requiresIfExists || []);
      requires.forEach(function(dep) {
        if (!visited.has(dep)) {
          var depModifier = map.get(dep);
          if (depModifier) {
            sort(depModifier);
          }
        }
      });
      result.push(modifier);
    }
    modifiers.forEach(function(modifier) {
      if (!visited.has(modifier.name)) {
        sort(modifier);
      }
    });
    return result;
  }
  function orderModifiers(modifiers) {
    var orderedModifiers = order(modifiers);
    return modifierPhases.reduce(function(acc, phase) {
      return acc.concat(orderedModifiers.filter(function(modifier) {
        return modifier.phase === phase;
      }));
    }, []);
  }

  // node_modules/@popperjs/core/lib/utils/debounce.js
  function debounce(fn2) {
    var pending;
    return function() {
      if (!pending) {
        pending = new Promise(function(resolve) {
          Promise.resolve().then(function() {
            pending = void 0;
            resolve(fn2());
          });
        });
      }
      return pending;
    };
  }

  // node_modules/@popperjs/core/lib/utils/mergeByName.js
  function mergeByName(modifiers) {
    var merged = modifiers.reduce(function(merged2, current) {
      var existing = merged2[current.name];
      merged2[current.name] = existing ? Object.assign({}, existing, current, {
        options: Object.assign({}, existing.options, current.options),
        data: Object.assign({}, existing.data, current.data)
      }) : current;
      return merged2;
    }, {});
    return Object.keys(merged).map(function(key) {
      return merged[key];
    });
  }

  // node_modules/@popperjs/core/lib/createPopper.js
  var DEFAULT_OPTIONS = {
    placement: "bottom",
    modifiers: [],
    strategy: "absolute"
  };
  function areValidElements() {
    for (var _len = arguments.length, args = new Array(_len), _key = 0; _key < _len; _key++) {
      args[_key] = arguments[_key];
    }
    return !args.some(function(element) {
      return !(element && typeof element.getBoundingClientRect === "function");
    });
  }
  function popperGenerator(generatorOptions) {
    if (generatorOptions === void 0) {
      generatorOptions = {};
    }
    var _generatorOptions = generatorOptions, _generatorOptions$def = _generatorOptions.defaultModifiers, defaultModifiers2 = _generatorOptions$def === void 0 ? [] : _generatorOptions$def, _generatorOptions$def2 = _generatorOptions.defaultOptions, defaultOptions = _generatorOptions$def2 === void 0 ? DEFAULT_OPTIONS : _generatorOptions$def2;
    return function createPopper2(reference2, popper2, options) {
      if (options === void 0) {
        options = defaultOptions;
      }
      var state = {
        placement: "bottom",
        orderedModifiers: [],
        options: Object.assign({}, DEFAULT_OPTIONS, defaultOptions),
        modifiersData: {},
        elements: {
          reference: reference2,
          popper: popper2
        },
        attributes: {},
        styles: {}
      };
      var effectCleanupFns = [];
      var isDestroyed = false;
      var instance = {
        state,
        setOptions: function setOptions(setOptionsAction) {
          var options2 = typeof setOptionsAction === "function" ? setOptionsAction(state.options) : setOptionsAction;
          cleanupModifierEffects();
          state.options = Object.assign({}, defaultOptions, state.options, options2);
          state.scrollParents = {
            reference: isElement(reference2) ? listScrollParents(reference2) : reference2.contextElement ? listScrollParents(reference2.contextElement) : [],
            popper: listScrollParents(popper2)
          };
          var orderedModifiers = orderModifiers(mergeByName([].concat(defaultModifiers2, state.options.modifiers)));
          state.orderedModifiers = orderedModifiers.filter(function(m) {
            return m.enabled;
          });
          runModifierEffects();
          return instance.update();
        },
        forceUpdate: function forceUpdate() {
          if (isDestroyed) {
            return;
          }
          var _state$elements = state.elements, reference3 = _state$elements.reference, popper3 = _state$elements.popper;
          if (!areValidElements(reference3, popper3)) {
            return;
          }
          state.rects = {
            reference: getCompositeRect(reference3, getOffsetParent(popper3), state.options.strategy === "fixed"),
            popper: getLayoutRect(popper3)
          };
          state.reset = false;
          state.placement = state.options.placement;
          state.orderedModifiers.forEach(function(modifier) {
            return state.modifiersData[modifier.name] = Object.assign({}, modifier.data);
          });
          for (var index = 0; index < state.orderedModifiers.length; index++) {
            if (state.reset === true) {
              state.reset = false;
              index = -1;
              continue;
            }
            var _state$orderedModifie = state.orderedModifiers[index], fn2 = _state$orderedModifie.fn, _state$orderedModifie2 = _state$orderedModifie.options, _options = _state$orderedModifie2 === void 0 ? {} : _state$orderedModifie2, name = _state$orderedModifie.name;
            if (typeof fn2 === "function") {
              state = fn2({
                state,
                options: _options,
                name,
                instance
              }) || state;
            }
          }
        },
        update: debounce(function() {
          return new Promise(function(resolve) {
            instance.forceUpdate();
            resolve(state);
          });
        }),
        destroy: function destroy() {
          cleanupModifierEffects();
          isDestroyed = true;
        }
      };
      if (!areValidElements(reference2, popper2)) {
        return instance;
      }
      instance.setOptions(options).then(function(state2) {
        if (!isDestroyed && options.onFirstUpdate) {
          options.onFirstUpdate(state2);
        }
      });
      function runModifierEffects() {
        state.orderedModifiers.forEach(function(_ref) {
          var name = _ref.name, _ref$options = _ref.options, options2 = _ref$options === void 0 ? {} : _ref$options, effect4 = _ref.effect;
          if (typeof effect4 === "function") {
            var cleanupFn = effect4({
              state,
              name,
              instance,
              options: options2
            });
            var noopFn = function noopFn2() {
            };
            effectCleanupFns.push(cleanupFn || noopFn);
          }
        });
      }
      function cleanupModifierEffects() {
        effectCleanupFns.forEach(function(fn2) {
          return fn2();
        });
        effectCleanupFns = [];
      }
      return instance;
    };
  }

  // node_modules/@popperjs/core/lib/popper.js
  var defaultModifiers = [eventListeners_default, popperOffsets_default, computeStyles_default, applyStyles_default, offset_default, flip_default, preventOverflow_default, arrow_default, hide_default];
  var createPopper = /* @__PURE__ */ popperGenerator({
    defaultModifiers
  });

  // frappe/public/js/frappe/ui/like.js
  frappe.ui.is_liked = function(doc) {
    return frappe.ui.get_liked_by(doc).includes(frappe.session.user);
  };
  frappe.ui.get_liked_by = function(doc) {
    return doc._liked_by ? JSON.parse(doc._liked_by) : [];
  };
  frappe.ui.toggle_like = function($btn, doctype, name, callback) {
    const add = $btn.hasClass("not-liked") ? "Yes" : "No";
    $btn.css("pointer-events", "none");
    frappe.call({
      method: "frappe.desk.like.toggle_like",
      quiet: true,
      args: {
        doctype,
        name,
        add
      },
      callback: function(r) {
        $btn.css("pointer-events", "auto");
        if (r.exc) {
          return;
        }
        $btn.toggleClass("not-liked", add === "No");
        $btn.toggleClass("liked", add === "Yes");
        const doc = locals[doctype] && locals[doctype][name];
        if (doc) {
          let liked_by = frappe.ui.get_liked_by(doc);
          if (add === "Yes" && !liked_by.includes(frappe.session.user)) {
            liked_by.push(frappe.session.user);
          }
          if (add === "No" && liked_by.includes(frappe.session.user)) {
            liked_by = liked_by.filter((user) => user !== frappe.session.user);
          }
          doc._liked_by = JSON.stringify(liked_by);
        }
        if (callback) {
          callback();
        }
      }
    });
  };
  frappe.ui.click_toggle_like = function() {
    console.warn("`frappe.ui.click_toggle_like` is deprecated and has no effect.");
  };
  frappe.ui.setup_like_popover = ($parent, selector) => {
    if (frappe.dom.is_touchscreen()) {
      return;
    }
    let active_target = null;
    let active_popover = null;
    let active_popper = null;
    let hide_timer = null;
    const clear_hide_timer = () => {
      if (hide_timer) {
        clearTimeout(hide_timer);
        hide_timer = null;
      }
    };
    const destroy_active_popover = () => {
      clear_hide_timer();
      if (active_target) {
        active_target.off(".likePopover");
      }
      if (active_popover) {
        active_popover.off(".likePopover");
        active_popover.remove();
      }
      if (active_popper) {
        active_popper.destroy();
      }
      active_target = null;
      active_popover = null;
      active_popper = null;
    };
    const schedule_hide = () => {
      clear_hide_timer();
      hide_timer = setTimeout(() => {
        destroy_active_popover();
      }, 120);
    };
    const get_liked_by_users = (target_element) => {
      let liked_by = target_element.parents(".liked-by").attr("data-liked-by");
      liked_by = liked_by ? decodeURI(liked_by) : "[]";
      return JSON.parse(liked_by);
    };
    const get_popover_content = (target_element) => {
      const liked_by = get_liked_by_users(target_element);
      const content = $('<div class="liked-by-popover-content"></div>');
      const like_count = liked_by.length;
      if (like_count > 3) {
        const like_summary = __("Liked by {0} people", [like_count]);
        const like_count_html = $(
          `<div class="liked-by-popover-summary">${like_summary}</div>`
        );
        content.append(like_count_html);
      }
      if (!liked_by.length) {
        return content;
      }
      const liked_by_list = $('<ul class="list-unstyled"></ul>');
      const link_base = "/desk/user/";
      liked_by.forEach((user) => {
        liked_by_list.append(`
				<li data-user=${user}>${frappe.avatar(user, "avatar-xs")}
					<span>${frappe.user.full_name(user)}</span>
				</li>
			`);
      });
      liked_by_list.children("li").on("click", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        const user = ev.currentTarget.dataset.user;
        setTimeout(() => destroy_active_popover(), 0);
        frappe.set_route(link_base + user);
      });
      content.append(liked_by_list);
      return content;
    };
    const show_popover = (target_element) => {
      if (!get_liked_by_users(target_element).length) {
        destroy_active_popover();
        return;
      }
      if ((active_target == null ? void 0 : active_target.get(0)) === target_element.get(0) && active_popover) {
        clear_hide_timer();
        active_popper == null ? void 0 : active_popper.update();
        return;
      }
      destroy_active_popover();
      const popover = $(
        `<div class="liked-by-popover popover show" role="tooltip">
				<div class="popover-body popover-content"></div>
			</div>`
      );
      popover.find(".popover-content").append(get_popover_content(target_element));
      $(document.body).append(popover);
      const popper2 = createPopper(target_element.get(0), popover.get(0), {
        placement: "bottom",
        modifiers: [
          {
            name: "offset",
            options: {
              offset: [0, 8]
            }
          },
          {
            name: "preventOverflow",
            options: {
              padding: 12
            }
          },
          {
            name: "flip",
            options: {
              padding: 12,
              fallbackPlacements: ["bottom-start", "top", "top-start"]
            }
          }
        ]
      });
      active_target = target_element;
      active_popover = popover;
      active_popper = popper2;
      target_element.off(".likePopover").on("mouseenter.likePopover", clear_hide_timer).on("mouseleave.likePopover", schedule_hide);
      popover.off(".likePopover").on("mousedown.likePopover click.likePopover", (ev) => {
        ev.stopPropagation();
      }).on("mouseenter.likePopover", clear_hide_timer).on("mouseleave.likePopover", schedule_hide);
    };
    $parent.on("mouseenter", selector, function() {
      show_popover($(this));
    });
    $parent.on("mouseleave", selector, schedule_hide);
  };

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/html/print_template.html
  frappe.templates["print_template"] = `<!DOCTYPE html>
<html lang="{{ lang }}" dir="{{ layout_direction }}">
<head>
	<meta charset="utf-8">
	<meta http-equiv="X-UA-Compatible" content="IE=edge">
	<meta name="viewport" content="width=device-width, initial-scale=1">
	<meta name="description" content="">
	<meta name="author" content="">
	<title>{{ title }}</title>
	<link href="{{ base_url }}{{ frappe.assets.bundled_asset('print.bundle.css', frappe.utils.is_rtl(lang)) }}" rel="stylesheet">
	<style>
		{{ print_css }}
	</style>
</head>
<body>
	<div class="print-format-gutter">
		<div class="print-format {% if landscape %}landscape{% endif %}"
				{% if columns.length > 20 %}
					{% if can_use_smaller_font %}
						style="font-size: 4.0pt"
					{% endif %}
				{% endif %}
			>
			{% if print_settings.letter_head %}
			<div {% if print_settings.repeat_header_footer %} id="header-html" class="hidden-pdf" {% endif %}>
				<div class="letter-head">{{ print_settings.letter_head.header }}</div>
			</div>
			{% endif %}
			{{ content }}
			{% if print_settings.repeat_header_footer %}
			<div id="footer-html" class="visible-pdf">
				{% if print_settings.letter_head && print_settings.letter_head.footer %}
					<div class="letter-head-footer">
						{{ print_settings.letter_head.footer }}
					</div>
				{% endif %}
				<p class="text-center small page-number visible-pdf">
					{{ __("Page {0} of {1}", [\`<span class="page"></span>\`, \`<span class="topage"></span>\`]) }}
				</p>
			</div>
			{% endif %}
		</div>
	</div>
</body>
</html>
`;

  // frappe/public/js/frappe/list/list_filter.js
  frappe.provide("frappe.ui");
  var ListFilter = class {
    constructor(list_view) {
      this.list_view = list_view;
      Object.assign(this, arguments[0]);
      this.can_add_global = frappe.user.has_role(["System Manager", "Administrator"]);
      this.filters = [];
      this.active_filter = null;
      this.refresh_list_filter();
    }
    refresh_list_filter() {
      if (frappe.is_mobile())
        return;
      this.get_list_filters().then(() => {
        this.render_saved_filters();
      });
      this.saved_filters_btn = this.list_view.page.add_inner_button(
        __("Filters"),
        [],
        __("Saved Filters")
      );
      const filter_x_btn = $(".filter-x-button");
      filter_x_btn.on("click", () => {
        this.active_filter = null;
        this.update_active_filter_label("Saved Filters");
      });
    }
    render_saved_filters() {
      const $menu = this.saved_filters_btn.parent();
      $menu.empty();
      this.filters.forEach((filter) => {
        const $item = this.filter_template(filter);
        $item.find(".dropdown-item").on("click", () => {
          this.apply_saved_filter(filter.name, filter.filter_name);
        });
        $item.find(".remove-filter").on("click", (e) => {
          e.preventDefault();
          e.stopPropagation();
          this.bind_remove_filter(filter);
        });
        $menu.append($item);
      });
      this.append_create_new_item($menu);
    }
    apply_saved_filter(filter_name, filter_label) {
      this.list_view.filter_area.clear().then(() => {
        this.list_view.filter_area.add(this.get_filters_values(filter_name));
        this.active_filter = filter_label;
        this.update_active_filter_label(this.active_filter);
      });
    }
    update_active_filter_label(label) {
      $(`.inner-group-button[data-label="${encodeURIComponent("Saved Filters")}"] button`).contents().first()[0].textContent = label;
    }
    bind_remove_filter(filter) {
      frappe.confirm(
        __("Are you sure you want to remove the {0} filter?", [filter.filter_name.bold()]),
        () => {
          const name = filter.name;
          const applied_filters = this.get_filters_values(name);
          this.remove_filter(name).then(() => this.refresh_list_filter());
          this.update_active_filter_label("Saved Filters");
          this.list_view.filter_area.remove_filters(applied_filters);
        }
      );
    }
    append_create_new_item($menu) {
      const new_filter = {
        name: "create_new",
        filter_name: "Save Current Filter"
      };
      const $create_item = this.filter_template(new_filter, true);
      $create_item.find(".filter-label").on("click", (e) => {
        this.show_create_filter_dialog();
      });
      $menu.append($create_item);
    }
    show_create_filter_dialog() {
      const fields = [
        {
          fieldname: "filter_name",
          label: __("Filter Name"),
          fieldtype: "Data",
          reqd: 1,
          description: __("Press Enter to save")
        }
      ];
      if (this.can_add_global) {
        fields.push({
          fieldname: "is_global",
          label: __("Is Global"),
          fieldtype: "Check",
          default: 0
        });
      }
      const dialog = new frappe.ui.Dialog({
        title: __("Create Saved Filter"),
        fields,
        primary_action_label: __("Create"),
        primary_action: (values) => {
          this.bind_save_filter(dialog, values.filter_name, values == null ? void 0 : values.is_global);
        }
      });
      dialog.show();
    }
    bind_save_filter(dialog, filter_name, is_global) {
      const value = filter_name;
      const has_value = Boolean(value);
      if (!has_value) {
        return;
      }
      if (this.filter_name_exists(value)) {
        $(dialog.fields_dict.filter_name.wrapper).addClass("has-error");
        dialog.fields_dict.filter_name.set_description(__("Duplicate Filter Name"));
        return;
      }
      this.save_filter(value, is_global).then(() => {
        this.refresh_list_filter();
        dialog.hide();
      });
    }
    save_filter(filter_name, is_global) {
      return frappe.db.insert({
        doctype: "List Filter",
        reference_doctype: this.list_view.doctype,
        filter_name,
        for_user: is_global ? "" : frappe.session.user,
        filters: JSON.stringify(this.get_current_filters())
      });
    }
    filter_template(filter, add_new = false) {
      return $(`
			<li class="saved-filter-item" data-name="${filter.name}">
				<a class="dropdown-item d-flex justify-content-between align-items-center">
					<span class="filter-label">
						${frappe.utils.escape_html(__(filter.filter_name))}
					</span>
					<span class="remove-filter ${add_new ? "d-none" : ""} ">
						${frappe.utils.icon("x", "sm")}
					</span>
				</a>
			</li>
		`);
    }
    remove_filter(name) {
      if (!name)
        return;
      return frappe.db.delete_doc("List Filter", name);
    }
    get_filters_values(name) {
      const filter = this.filters.find((filter2) => filter2.name === name);
      return JSON.parse(filter.filters || "[]");
    }
    get_current_filters() {
      return this.list_view.filter_area.get();
    }
    filter_name_exists(filter_name) {
      return (this.filters || []).find((f) => f.filter_name === filter_name);
    }
    get_list_filters() {
      if (frappe.session.user === "Guest")
        return Promise.resolve();
      return frappe.db.get_list("List Filter", {
        fields: ["name", "filter_name", "for_user", "filters"],
        filters: { reference_doctype: this.list_view.doctype },
        or_filters: [
          ["for_user", "=", frappe.session.user],
          ["for_user", "=", ""]
        ],
        order_by: "filter_name asc"
      }).then((filters) => {
        this.filters = filters || [];
      });
    }
  };

  // frappe/public/js/frappe/list/base_list.js
  frappe.provide("frappe.views");
  frappe.views.BaseList = class BaseList {
    constructor(opts) {
      Object.assign(this, opts);
    }
    show() {
      return frappe.run_serially([
        () => this.show_skeleton(),
        () => this.fetch_meta(),
        () => this.hide_skeleton(),
        () => this.check_permissions(),
        () => this.init(),
        () => this.before_refresh(),
        () => this.refresh(),
        () => this.setup_list_filter_by()
      ]);
    }
    init() {
      if (this.init_promise)
        return this.init_promise;
      let tasks = [
        this.setup_defaults,
        this.set_stats,
        this.setup_fields,
        this.setup_page,
        this.setup_main_section,
        this.setup_view,
        this.setup_view_menu
      ].map((fn2) => fn2.bind(this));
      this.init_promise = frappe.run_serially(tasks);
      return this.init_promise;
    }
    setup_defaults() {
      this.page_name = frappe.get_route_str();
      this.page_title = this.page_title || frappe.router.doctype_layout || __(this.doctype);
      this.meta = frappe.get_meta(this.doctype);
      this.settings = frappe.listview_settings[this.doctype] || {};
      this.user_settings = frappe.get_user_settings(this.doctype);
      this.start = 0;
      this.page_length = frappe.is_large_screen() ? 100 : 20;
      this.selected_page_count = this.page_length;
      this.data = [];
      this.method = "frappe.desk.reportview.get";
      this.can_create = frappe.model.can_create(this.doctype);
      this.can_write = frappe.model.can_write(this.doctype);
      this.fields = [];
      this.filters = [];
      this.sort_by = this.meta.sort_field || "creation";
      this.sort_order = this.meta.sort_order || "desc";
      this.primary_action = null;
      this.secondary_action = null;
      this.menu_items = [
        {
          label: __("Refresh"),
          action: () => this.refresh(),
          class: "visible-xs"
        }
      ];
    }
    get_list_view_settings() {
      return frappe.call("frappe.desk.listview.get_list_settings", {
        doctype: this.doctype
      }).then((doc) => this.list_view_settings = doc.message || {});
    }
    async setup_fields() {
      await this.set_fields();
      this.build_fields();
    }
    async set_fields() {
      let fields = [].concat(frappe.model.std_fields_list, this.meta.title_field);
      fields.forEach((f) => this._add_field(f));
    }
    get_fields_in_list_view() {
      return this.meta.fields.filter((df) => {
        return frappe.model.is_value_type(df.fieldtype) && df.in_list_view && frappe.perm.has_perm(this.doctype, df.permlevel, "read") || df.fieldtype === "Currency" && df.options && !df.options.includes(":") || df.fieldname === "status";
      });
    }
    build_fields() {
      this.fields = this.fields.map((f) => {
        if (typeof f === "string") {
          f = [f, this.doctype];
        }
        return f;
      });
      this.fields = this.fields.filter(Boolean);
      this.fields = this.fields.uniqBy((f) => f[0] + f[1]);
    }
    _add_field(fieldname, doctype) {
      var _a3;
      if (!fieldname)
        return;
      if (!doctype)
        doctype = this.doctype;
      if (typeof fieldname === "object") {
        const df = fieldname;
        fieldname = df.fieldname;
        doctype = df.parent || doctype;
      }
      if (!this.fields)
        this.fields = [];
      const is_valid_field = frappe.model.std_fields_list.includes(fieldname) || frappe.meta.has_field(doctype, fieldname) || fieldname === "_seen";
      let is_virtual = (_a3 = this.meta.fields.find((df) => df.fieldname == fieldname)) == null ? void 0 : _a3.is_virtual;
      if (!is_valid_field || is_virtual) {
        return;
      }
      this.fields.push([fieldname, doctype]);
    }
    set_stats() {
      this.stats = ["_user_tags"];
      this.workflow_state_fieldname = frappe.workflow.get_state_fieldname(this.doctype);
      if (this.workflow_state_fieldname) {
        if (!frappe.workflow.workflows[this.doctype]["override_status"]) {
          this._add_field(this.workflow_state_fieldname);
        }
        this.stats.push(this.workflow_state_fieldname);
      }
    }
    fetch_meta() {
      return frappe.model.with_doctype(this.doctype);
    }
    show_skeleton() {
    }
    hide_skeleton() {
    }
    check_permissions() {
      return true;
    }
    setup_page() {
      this.page = this.parent.page;
      this.$page = $(this.parent);
      this.page.main.addClass("layout-main-list");
      this.page.page_form.removeClass("row").addClass("flex");
      this.hide_page_form && this.page.page_form.hide();
      this.setup_page_head();
    }
    setup_page_head() {
      this.set_breadcrumbs();
      this.set_title();
      this.set_menu_items();
    }
    set_title() {
      var _a3;
      this.page.set_title(this.page_title, null, true, "", (_a3 = this.meta) == null ? void 0 : _a3.description);
    }
    setup_view_menu() {
      if (frappe.boot.desk_settings.view_switcher && !this.meta.force_re_route_to_default_view) {
        const icon_map = {
          Image: "image",
          List: "list",
          Report: "sheet",
          Calendar: "calendar",
          Gantt: "gantt",
          Kanban: "kanban",
          Dashboard: "dashboard",
          Map: "map"
        };
        const label_map = {
          List: __("List View"),
          Report: __("Report View"),
          Dashboard: __("Dashboard View"),
          Gantt: __("Gantt View"),
          Kanban: __("Kanban View"),
          Calendar: __("Calendar View"),
          Image: __("Image View"),
          Inbox: __("Inbox View"),
          Tree: __("Tree View"),
          Map: __("Map View")
        };
        this.views_menu = this.page.add_custom_button_group(
          label_map[this.view_name] || label_map["List"],
          icon_map[this.view_name] || "list"
        );
        this.views_list = new frappe.views.ListViewSelect({
          doctype: this.doctype,
          parent: this.views_menu,
          page: this.page,
          list_view: this,
          icon_map,
          label_map
        });
      }
    }
    set_default_secondary_action() {
      if (this.secondary_action) {
        const $secondary_action = this.page.set_secondary_action(
          this.secondary_action.label,
          this.secondary_action.action,
          this.secondary_action.icon
        );
        if (!this.secondary_action.icon) {
          $secondary_action.addClass("hidden-xs");
        } else if (!this.secondary_action.label) {
          $secondary_action.addClass("visible-xs");
        }
      } else {
        this.refresh_button = this.page.add_action_icon(
          "es-line-reload",
          () => {
            this.refresh();
          },
          "",
          __("Reload List")
        );
      }
    }
    set_menu_items() {
      this.set_default_secondary_action();
      this.menu_items && this.menu_items.map((item) => {
        if (item.condition && item.condition() === false) {
          return;
        }
        const $item = this.page.add_menu_item(
          item.label,
          item.action,
          item.standard,
          item.shortcut
        );
        if (item.class) {
          $item && $item.addClass(item.class);
        }
      });
    }
    set_breadcrumbs() {
      frappe.breadcrumbs.add(this.meta.module, this.doctype);
    }
    hide_sidebar() {
      $(document.body).toggleClass("no-list-sidebar", true);
    }
    setup_main_section() {
      return frappe.run_serially(
        [
          this.setup_list_wrapper,
          this.hide_sidebar,
          this.setup_filter_area,
          this.setup_sort_selector,
          this.setup_result_container_area,
          this.setup_result_area,
          this.setup_no_result_area,
          this.setup_freeze_area,
          this.setup_paging_area
        ].map((fn2) => fn2.bind(this))
      );
    }
    setup_list_wrapper() {
      this.$frappe_list = $('<div class="frappe-list">').appendTo(this.page.main);
    }
    setup_filter_area() {
      if (this.hide_filters)
        return;
      this.filter_area = new FilterArea(this);
      if (this.filters && this.filters.length > 0) {
        return this.filter_area.set(this.filters).catch(() => {
          this.filter_area.clear(false);
        });
      }
    }
    setup_sort_selector() {
      if (this.hide_sort_selector)
        return;
      this.sort_selector = new frappe.ui.SortSelector({
        parent: this.$filter_section,
        doctype: this.doctype,
        args: {
          sort_by: this.sort_by,
          sort_order: this.sort_order
        },
        onchange: this.on_sort_change.bind(this)
      });
    }
    on_sort_change() {
      this.refresh();
    }
    setup_result_container_area() {
      if (this.view == "List") {
        this.$frappe_list.append($(`<div class="result-container">`));
      }
    }
    setup_result_area() {
      this.$result = $(`<div class="result">`);
      let frappe_list = this.$frappe_list;
      if (this.view == "List") {
        frappe_list = this.$frappe_list.find(".result-container");
      }
      frappe_list.append(this.$result);
    }
    setup_no_result_area() {
      this.$no_result = $(`
			<div class="no-result text-muted flex justify-center align-center">
				${this.get_no_result_message()}
			</div>
		`).hide();
      this.$frappe_list.append(this.$no_result);
    }
    setup_freeze_area() {
      this.$freeze = $('<div class="freeze"></div>').hide();
      this.$frappe_list.append(this.$freeze);
    }
    get_no_result_message() {
      return __("Nothing to show");
    }
    setup_paging_area() {
      const paging_values = [20, 100, 500, 2500];
      this.$paging_area = $(
        `<div class="list-paging-area level">
				<div class="level-left">
					<div class="btn-group">
						${paging_values.map(
          (value) => `
							<button type="button" class="btn btn-default btn-sm btn-paging"
								data-value="${value}">
								${value}
							</button>
						`
        ).join("")}
					</div>
				</div>
				<div class="level-right">
					<button class="btn btn-default btn-more btn-sm">
						${__("Load More")}
					</button>
				</div>
			</div>`
      ).hide();
      this.$frappe_list.append(this.$paging_area);
      this.$paging_area.find(`.btn-paging[data-value="${this.page_length}"]`).addClass("btn-info").prop("disabled", true);
      this.$paging_area.on("click", ".btn-paging", (e) => {
        const $this = $(e.currentTarget);
        this.$paging_area.find(".btn-paging").removeClass("btn-info").prop("disabled", false);
        $this.addClass("btn-info").prop("disabled", true);
        const old_page_length = this.page_length;
        const new_page_length = $this.data().value;
        this.selected_page_count = new_page_length;
        if (this.page_length > new_page_length) {
          this.start = 0;
          this.page_length = new_page_length;
        } else {
          this.start = this.page_length;
          this.page_length = new_page_length - this.page_length;
        }
        if (old_page_length !== new_page_length) {
          this.refresh();
        }
      });
      this.$paging_area.on("click", ".btn-more", (e) => {
        this.start = this.data.length;
        this.page_length = this.selected_page_count;
        this.refresh();
      });
    }
    set_result_height() {
      if (this.view !== "List")
        return;
      this.$result[0].style.removeProperty("height");
      let resultContainerHeight = window.innerHeight - this.$paging_area.get(0).offsetHeight;
      if (!frappe.is_mobile()) {
        resultContainerHeight = resultContainerHeight - this.$result.get(0).offsetTop;
      }
      this.$result.parent(".result-container").css({
        height: resultContainerHeight - (frappe.is_mobile() ? 100 : 0) + "px"
      });
      this.$result[0].style.height = Math.max(this.$result[0].offsetHeight, resultContainerHeight) + "px";
      this.$no_result.css({
        height: window.innerHeight - this.$no_result.get(0).offsetTop + "px"
      });
    }
    get_fields() {
      return this.fields.map((f) => frappe.model.get_full_column_name(f[0], f[1]));
    }
    get_group_by() {
      let name_field = this.fields && this.fields.find((f) => f[0] == "name");
      if (name_field) {
        return frappe.model.get_full_column_name(name_field[0], name_field[1]);
      }
      return null;
    }
    setup_view() {
    }
    get_filter_value(fieldname) {
      var _a3;
      const filter = this.get_filters_for_args().filter((f) => f[1] == fieldname)[0];
      if (!filter)
        return;
      if (filter[2] === "like")
        return (_a3 = filter[3]) == null ? void 0 : _a3.replace(/^%?|%$/g, "");
      else if (filter[2] === "not set")
        return null;
      else
        return filter[3];
    }
    get_filters_for_args() {
      return this.filter_area ? this.filter_area.get().map((filter) => filter.slice(0, 4)) : [];
    }
    get_args() {
      let filters = this.get_filters_for_args();
      let group_by = this.get_group_by();
      let group_by_required = Array.isArray(filters) && filters.some((filter) => {
        return filter[0] !== this.doctype;
      });
      return {
        doctype: this.doctype,
        fields: this.get_fields(),
        filters,
        order_by: this.sort_selector && this.sort_selector.get_sql_string(),
        start: this.start,
        page_length: this.page_length,
        view: this.view,
        group_by: group_by_required ? group_by : null
      };
    }
    get_call_args() {
      const args = this.get_args();
      return {
        method: this.method,
        args,
        freeze: this.freeze_on_refresh || false,
        freeze_message: this.freeze_message || __("Loading") + "..."
      };
    }
    before_refresh() {
    }
    refresh() {
      let args = this.get_call_args();
      if (this.no_change(args)) {
        return Promise.resolve();
      }
      this.freeze(true);
      return frappe.call(args).then((r) => {
        this.prepare_data(r);
        this.toggle_result_area();
        this.before_render();
        this.render();
        this.after_render();
        this.set_result_height();
        this.freeze(false);
        this.reset_defaults();
        if (this.settings.refresh) {
          this.settings.refresh(this);
        }
      });
    }
    no_change(args) {
      if (this.last_args && JSON.stringify(args) === this.last_args) {
        return true;
      }
      this.last_args = JSON.stringify(args);
      setTimeout(() => {
        this.last_args = null;
      }, 3e3);
      return false;
    }
    prepare_data(r) {
      let data = r.message || {};
      Object.assign(frappe.boot.user_info, data.user_info);
      delete data.user_info;
      data = !Array.isArray(data) ? frappe.utils.dict(data.keys, data.values) : data;
      if (this.start === 0) {
        this.data = data;
      } else {
        this.data = this.data.concat(data);
      }
      this.data = this.data.uniqBy((d) => d.name);
    }
    reset_defaults() {
      this.page_length = this.page_length + this.start;
      this.start = 0;
    }
    freeze() {
    }
    before_render() {
    }
    after_render() {
    }
    render() {
    }
    on_filter_change() {
    }
    toggle_result_area() {
      this.$result.parent(".result-container").toggle(this.data.length > 0);
      this.$result.toggle(this.data.length > 0);
      this.$paging_area.toggle(this.data.length > 0);
      this.$no_result.toggle(this.data.length == 0);
      if (this.data.length) {
        const show_more = this.start + this.page_length <= this.data.length;
        this.$paging_area.find(".btn-more").toggle(show_more);
      }
    }
    call_for_selected_items(method, args = {}) {
      args.names = this.get_checked_items(true);
      frappe.call({
        method,
        args,
        freeze: true,
        callback: (r) => {
          if (!r.exc) {
            this.refresh();
          }
        }
      });
    }
    setup_list_filter_by() {
      new ListFilter(this);
    }
  };
  var FilterArea = class {
    constructor(list_view) {
      this.list_view = list_view;
      this.list_view.page.page_form.append(`<div class="standard-filter-section flex"></div>`);
      const filter_area = this.list_view.hide_page_form ? this.list_view.page.custom_actions : this.list_view.page.page_form;
      this.list_view.$filter_section = $('<div class="filter-section flex">').appendTo(
        filter_area
      );
      this.$filter_list_wrapper = this.list_view.$filter_section;
      this.trigger_refresh = true;
      this.debounced_refresh_list_view = frappe.utils.debounce(
        this.refresh_list_view.bind(this),
        300
      );
      this.setup();
      if (frappe.is_mobile())
        this.setup_mobile(list_view);
    }
    setup_mobile(list_view) {
      var _a3;
      const me2 = this;
      this.standard_filters_visible = false;
      (_a3 = this.standard_filters_wrapper) == null ? void 0 : _a3.hide();
      this.list_view.page.page_form.css("justify-content", "flex-end");
      list_view.page.page_form.addClass("flex-column");
      this.$filter_list_wrapper.addClass("justify-between p-0");
      this.$filter_list_wrapper.find(".filter-selector").css("margin", "0 0 0 auto");
      $(`<button class="filter-toggle btn btn-default btn-sm filter-button">
					<span class="filter-icon button-icon">
						${frappe.utils.icon("chevrons-up-down")}
					</span>
				</button>
			</div>`).prependTo(this.$filter_list_wrapper.find(".filter-selector")).on("click", function() {
        me2.toggle_standard_filter();
      });
      let children = list_view.page.page_form.children();
      list_view.page.page_form.append(children.get().reverse());
    }
    toggle_standard_filter() {
      if (this.standard_filters_visible) {
        this.standard_filters_visible = false;
        this.standard_filters_wrapper.hide();
      } else {
        this.standard_filters_visible = true;
        this.standard_filters_wrapper.show();
      }
    }
    setup() {
      var _a3;
      if (!this.list_view.hide_page_form)
        this.make_standard_filters();
      this.make_filter_list();
      this.user_setting_fields = ((_a3 = frappe.get_user_settings(this.list_view.doctype)) == null ? void 0 : _a3.group_by_fields) || [];
      if (["assigned_to", "owner", "tags"].some((v) => this.user_setting_fields.includes(v))) {
        this.render_non_standard_fields_filter();
      }
    }
    get() {
      let filters = this.filter_list.get_filters();
      let standard_filters = this.get_standard_filters();
      return filters.concat(standard_filters).uniqBy(JSON.stringify);
    }
    set(filters) {
      this.trigger_refresh = false;
      return this.add(filters, false).then(() => {
        this.trigger_refresh = true;
        this.filter_list.update_filter_button();
      });
    }
    add(filters, refresh = true) {
      if (!filters || Array.isArray(filters) && filters.length === 0)
        return Promise.resolve();
      if (typeof filters[0] === "string") {
        const filter = Array.from(arguments);
        filters = [filter];
      }
      filters = filters.filter((f) => !this.exists(f));
      const { non_standard_filters, promise } = this.set_standard_filter(filters);
      return promise.then(() => {
        return non_standard_filters.length > 0 && this.filter_list.add_filters(non_standard_filters);
      }).then(() => {
        refresh && this.list_view.refresh();
      });
    }
    refresh_list_view() {
      if (this.trigger_refresh) {
        this.list_view.start = 0;
        this.list_view.refresh();
        this.list_view.on_filter_change();
      }
    }
    exists(f) {
      let exists = false;
      const fields_dict = this.list_view.page.fields_dict;
      if (f[2] === "=" && f[1] in fields_dict) {
        const value = fields_dict[f[1]].get_value();
        if (value) {
          exists = true;
        }
      }
      if (!exists) {
        exists = this.filter_list.filter_exists(f);
      }
      return exists;
    }
    set_standard_filter(filters) {
      if (filters.length === 0) {
        return {
          non_standard_filters: [],
          promise: Promise.resolve()
        };
      }
      const fields_dict = this.list_view.page.fields_dict;
      return filters.reduce((out, filter) => {
        var _a3, _b, _c, _d;
        const [dt, fieldname, condition, value] = filter;
        out.promise = out.promise || Promise.resolve();
        out.non_standard_filters = out.non_standard_filters || [];
        if (fields_dict[fieldname] && (condition === "=" || condition === "like" && ((_b = (_a3 = fields_dict[fieldname]) == null ? void 0 : _a3.df) == null ? void 0 : _b.fieldtype) != "Link" || condition === "descendants of (inclusive)" && ((_d = (_c = fields_dict[fieldname]) == null ? void 0 : _c.df) == null ? void 0 : _d.fieldtype) == "Link")) {
          out.promise = out.promise.then(() => {
            if (fields_dict[fieldname].df) {
              fields_dict[fieldname].df.match_type = condition;
            }
            return fields_dict[fieldname].set_value(value);
          });
        } else {
          out.non_standard_filters.push(filter);
        }
        return out;
      }, {});
    }
    render_non_standard_fields_filter() {
      let get_item_html = (fieldname) => {
        let label, fieldtype;
        if (fieldname === "assigned_to") {
          label = __("Assigned To");
        } else if (fieldname === "owner") {
          label = __("Created By");
        } else if (fieldname === "tags") {
          label = __("Tags");
        }
        return `<div class="group-by-field list-link form-group frappe-control input-max-width">
						<a class="btn btn-default btn-sm flex justify-between list-sidebar-button w-100" data-toggle="dropdown"
						aria-haspopup="true" aria-expanded="false"
						data-label="${label}" data-fieldname="${fieldname}" data-fieldtype="${fieldtype}"
						href="#" onclick="return false;">
							<span class="ellipsis">${__(label)}</span>
							<span>${frappe.utils.icon("select", "xs")}</span>
						</a>
					<ul class="dropdown-menu group-by-dropdown" role="menu">
					</ul>
			</div>`;
      };
      let filtes_to_add = [];
      if (this.user_setting_fields.includes("owner")) {
        filtes_to_add.push("owner");
      }
      if (this.user_setting_fields.includes("assigned_to")) {
        filtes_to_add.push("assigned_to");
      }
      if (this.user_setting_fields.includes("tags")) {
        filtes_to_add.push("tags");
      }
      let html = filtes_to_add.map(get_item_html).join("");
      this.list_view.page.page_form.find(".standard-filter-section").append(html);
      this.setup_non_standard_items_dropdown();
      this.setup_filter_by();
    }
    setup_non_standard_items_dropdown() {
      let standard_filter_container = this.list_view.page.page_form.find(
        ".standard-filter-section"
      );
      standard_filter_container.find(".group-by-field").on("show.bs.dropdown", (e) => {
        let $dropdown = $(e.currentTarget).find(".group-by-dropdown");
        this.set_dropdown_loading_state($dropdown);
        let fieldname = $(e.currentTarget).find("a").attr("data-fieldname");
        let fieldtype = $(e.currentTarget).find("a").attr("data-fieldtype");
        if (fieldname == "tags") {
          $dropdown.addClass("list-stats-dropdown");
          this.get_stats($dropdown);
          return;
        }
        this.get_group_by_count(fieldname).then((field_count_list) => {
          if (field_count_list.length) {
            if (fieldname == "assigned_to") {
              fieldname = "_assign";
            }
            if (fieldname == "tags") {
              fieldname = "_user_tags";
            }
            let applied_filter = this.list_view.get_filter_value(fieldname);
            this.render_dropdown_items(
              field_count_list,
              fieldtype,
              $dropdown,
              applied_filter
            );
            this.setup_search($dropdown);
          } else {
            this.set_empty_state($dropdown);
          }
        });
      });
    }
    setup_filter_by() {
      let standard_filter_container = this.list_view.page.page_form.find(
        ".standard-filter-section"
      );
      standard_filter_container.on("click", ".group-by-item", (e) => {
        let $target = $(e.currentTarget);
        let is_selected = $target.hasClass("selected");
        let fieldname = $target.parents(".group-by-field").find("a").data("fieldname");
        let value = typeof $target.data("value") === "string" ? decodeURIComponent($target.data("value").trim()) : $target.data("value");
        if (fieldname == "assigned_to") {
          fieldname = "_assign";
        }
        if (fieldname == "tags") {
          fieldname = "_user_tags";
        }
        return this.list_view.filter_area.remove(fieldname).then(() => {
          if (is_selected)
            return;
          return this.apply_filter(fieldname, value);
        });
      });
    }
    render_dropdown_items(fields, fieldtype, $dropdown, applied_filter) {
      let standard_html = `
			<div class="dropdown-search mb-1">
				<input type="text"
					placeholder="${__("Search")}"
					data-element="search"
					class="dropdown-search-input form-control input-xs"
				>
			</div>
		`;
      let applied_filter_html = "";
      let dropdown_items_html = "";
      fields.map((field) => {
        if (field.name === applied_filter) {
          applied_filter_html = this.get_dropdown_html(field, fieldtype, true);
        } else {
          dropdown_items_html += this.get_dropdown_html(field, fieldtype);
        }
      });
      let dropdown_html = standard_html + applied_filter_html + dropdown_items_html;
      $dropdown.toggleClass("has-selected", Boolean(applied_filter_html));
      $dropdown.html(dropdown_html);
    }
    get_dropdown_html(field, fieldtype, applied = false) {
      let label;
      if (field.name == null) {
        label = __("Not Set");
      } else if (field.name === frappe.session.user) {
        label = __("Me");
      } else if (fieldtype && fieldtype == "Check") {
        label = field.name == "0" ? __("No") : __("Yes");
      } else if (fieldtype && fieldtype == "Link" && field.title) {
        label = __(field.title);
      } else {
        label = __(field.name);
      }
      let value = field.name == null ? "" : encodeURIComponent(field.name);
      let applied_html = applied ? `<span class="applied"> ${frappe.utils.icon("tick", "xs")} </span>` : "";
      return `<div class="group-by-item ${applied ? "selected" : ""}" data-value="${value}">
			<a class="dropdown-item flex justify-between" href="#" onclick="return false;">
				<span class="group-by-value ellipsis" data-name="${field.name}">
					${applied_html}
					${label}
				</span>
				<span class="group-by-count">${field.count}</span>
			</a>
		</div>`;
    }
    get_stats($dropdown) {
      let me2 = this;
      frappe.call({
        method: "frappe.desk.reportview.get_sidebar_stats",
        type: "GET",
        args: {
          stats: ["_user_tags"],
          doctype: me2.list_view.doctype,
          filters: (me2.list_view.filter_area ? me2.list_view.get_filters_for_args() : me2.default_filters) || []
        },
        callback: function(r) {
          let stats = (r.message.stats || {})["_user_tags"] || [];
          me2.render_stat(stats, $dropdown);
          frappe.utils.setup_search($dropdown, ".stat-link", ".stat-label");
        }
      });
    }
    render_stat(stats, $dropdown) {
      let args = {
        stats,
        label: __("Tags"),
        applied_filter: this.list_view.get_filter_value("_user_tags")
      };
      let tag_list = $(frappe.render_template("list_sidebar_stat", args)).on(
        "click",
        ".stat-link",
        (e) => {
          let fieldname = $(e.currentTarget).attr("data-field");
          let label = $(e.currentTarget).attr("data-label");
          let condition = "like";
          let existing = this.list_view.filter_area.filter_list.get_filter(fieldname);
          if (existing) {
            existing.remove();
          }
          if (label == "No Tags") {
            label = "not set";
            condition = "is";
          }
          this.list_view.filter_area.add(this.doctype, fieldname, condition, label);
        }
      );
      $dropdown.html(tag_list);
    }
    get_group_by_count(field) {
      let current_filters = this.list_view.get_filters_for_args();
      current_filters = current_filters.filter(
        (f_arr) => !f_arr.includes(field === "assigned_to" ? "_assign" : field)
      );
      let args = {
        doctype: this.list_view.doctype,
        current_filters,
        field
      };
      return frappe.call("frappe.desk.listview.get_group_by_count", args).then((r) => {
        let field_counts = r.message || [];
        field_counts = field_counts.filter((f) => f.count !== 0);
        let current_user = field_counts.find((f) => f.name === frappe.session.user);
        field_counts = field_counts.filter(
          (f) => !["Guest", "Administrator", frappe.session.user].includes(f.name)
        );
        if (current_user)
          field_counts.unshift(current_user);
        return field_counts;
      });
    }
    apply_filter(fieldname, value) {
      let operator = "=";
      if (value === "" || fieldname === "_user_tags" && value === __("No Tags")) {
        operator = "is";
        value = "not set";
      }
      if (fieldname === "_assign") {
        operator = "like";
        value = `%${value}%`;
      }
      return this.list_view.filter_area.add(this.list_view.doctype, fieldname, operator, value);
    }
    set_dropdown_loading_state($dropdown) {
      $dropdown.html(`<li>
			<div class="empty-state group-by-loading">
				${__("Loading...")}
			</div>
		</li>`);
    }
    setup_search($dropdown) {
      frappe.utils.setup_search($dropdown, ".group-by-item", ".group-by-value", "data-name");
    }
    set_empty_state($dropdown) {
      $dropdown.html(
        `<div class="empty-state group-by-empty">
				${__("No filters found")}
			</div>`
      );
    }
    remove_filters(filters) {
      filters.map((f) => {
        this.remove(f[1]);
      });
    }
    remove(fieldname) {
      const fields_dict = this.list_view.page.fields_dict;
      if (fieldname in fields_dict) {
        fields_dict[fieldname].set_value("");
      }
      let filter = this.filter_list.get_filter(fieldname);
      if (filter)
        filter.remove();
      this.filter_list.apply();
      return Promise.resolve();
    }
    clear(refresh = true) {
      if (!refresh) {
        this.trigger_refresh = false;
      }
      this.filter_list.clear_filters();
      const promises = [];
      const fields_dict = this.list_view.page.fields_dict;
      for (let key in fields_dict) {
        const field = this.list_view.page.fields_dict[key];
        promises.push(() => field.set_value(""));
      }
      return frappe.run_serially(promises).then(() => {
        this.trigger_refresh = true;
        if (promises.length === 0) {
          this.debounced_refresh_list_view();
        }
      });
    }
    async make_standard_filters() {
      var _a3;
      this.standard_filters_wrapper = this.list_view.page.page_form.find(
        ".standard-filter-section"
      );
      let fields = [];
      if (!this.list_view.settings.hide_name_filter) {
        let field = {
          fieldtype: "Data",
          label: "ID",
          condition: "like",
          fieldname: "name",
          onchange: () => this.debounced_refresh_list_view()
        };
        if (frappe.is_mobile()) {
          let mobile_id_filter = this.$filter_list_wrapper.append(
            `<div class="mobile-id-filter"></div>`
          );
          this.list_view.page.add_field(field, mobile_id_filter.find(".mobile-id-filter"));
        } else {
          fields.push(field);
        }
      }
      if (this.list_view.custom_filter_configs || this.list_view.settings.custom_filter_configs) {
        const custom_filter_configs = this.list_view.custom_filter_configs || this.list_view.settings.custom_filter_configs;
        await Promise.resolve(
          typeof custom_filter_configs === "function" ? custom_filter_configs() : custom_filter_configs
        ).then((configs) => {
          configs.forEach((config) => {
            config.onchange = () => this.debounced_refresh_list_view();
          });
          fields = fields.concat(configs);
        });
      }
      const doctype_fields = this.list_view.meta.fields;
      const title_field = this.list_view.meta.title_field;
      const user_setting_fields = ((_a3 = frappe.get_user_settings(this.list_view.doctype)) == null ? void 0 : _a3.group_by_fields) || [];
      fields = fields.concat(
        doctype_fields.filter(
          (df) => (df.fieldname === title_field || (df.in_standard_filter || user_setting_fields.includes(df.fieldname)) && frappe.model.is_value_type(df.fieldtype)) && frappe.perm.has_perm(this.list_view.doctype, df.permlevel)
        ).map((df) => {
          let options = df.options;
          let condition = "=";
          let fieldtype = df.fieldtype;
          if ([
            "Text",
            "Small Text",
            "Text Editor",
            "HTML Editor",
            "Data",
            "Code",
            "Phone",
            "JSON",
            "Read Only"
          ].includes(fieldtype)) {
            fieldtype = "Data";
            condition = "like";
          }
          if (df.fieldtype == "Select" && df.options) {
            options = df.options.split("\n");
            if (options.length > 0 && options[0] != "") {
              options.unshift("");
              options = options.join("\n");
            }
          }
          if (df.fieldtype == "Link" && df.options && frappe.boot.treeviews.includes(df.options)) {
            condition = "descendants of (inclusive)";
          }
          return {
            fieldtype,
            label: __(df.label, null, df.parent),
            options,
            fieldname: df.fieldname,
            condition,
            onchange: () => this.debounced_refresh_list_view(),
            ignore_link_validation: fieldtype === "Dynamic Link",
            is_filter: 1
          };
        })
      );
      fields.sort((a, b) => {
        if (a.fieldtype === "Check" && b.fieldtype !== "Check") {
          return 1;
        } else if (a.fieldtype !== "Check" && b.fieldtype === "Check") {
          return -1;
        } else {
          return 0;
        }
      });
      fields.map((df) => {
        this.list_view.page.add_field(df, this.standard_filters_wrapper);
        const input_fieldtypes = [
          "Data",
          "Text",
          "Small Text",
          "Long Text",
          "Code",
          "Phone",
          "Read Only",
          "Barcode"
        ];
        if (input_fieldtypes.includes(df.fieldtype)) {
          df.match_type = df.condition || "=";
          this.filter_field_with_match_type(df);
        }
      });
    }
    filter_field_with_match_type(df) {
      setTimeout(() => {
        const field = this.list_view.page.fields_dict[df.fieldname];
        if (!field || !field.$wrapper)
          return;
        const $input = field.$wrapper.find("input").first();
        if (!$input.length || $input.closest(".input-group").length)
          return;
        const getIcon = (match_type) => {
          if (match_type === "=") {
            return frappe.utils.icon("equal");
          } else {
            return frappe.utils.icon("equal-approximately");
          }
        };
        $input.wrap('<div class="input-group"></div>');
        const $inputGroup = $input.parent();
        const $dropdown = $(`
			<div class="input-group-btn mr-0">
				<button type="button"
					class="btn btn-default  match-type-dropdown-btn"
					data-toggle="dropdown"
					aria-haspopup="true"
					aria-expanded="false">
					${getIcon(df.match_type || "\u2248")}

				</button>
				<ul class="dropdown-menu match-type-dropdown-menu dropdown-menu-right">
					<li class="dropdown-item" data-match-type="=">${__("Equals")}</li>
					<li class="dropdown-item" data-match-type="like">${__("Like")}</li>
				</ul>
			</div>
		`);
        $inputGroup.append($dropdown);
        $dropdown.find(".dropdown-item").on("click", (e) => {
          var _a3;
          e.preventDefault();
          e.stopPropagation();
          $dropdown.find("button").dropdown("toggle");
          const new_type = $(e.currentTarget).data("match-type");
          const current_type = field.df.match_type || "\u2248";
          if (new_type === current_type)
            return;
          field.df.match_type = new_type;
          $dropdown.find("button").html(getIcon(new_type));
          let value = (_a3 = field.get_value) == null ? void 0 : _a3.call(field);
          if (new_type === "=" && value) {
            field.set_value(value.replace(/^%+|%+$/g, ""));
          }
          if (value) {
            this.debounced_refresh_list_view();
          }
        });
      }, 100);
    }
    get_standard_filters() {
      const filters = [];
      const fields_dict = this.list_view.page.fields_dict;
      for (let key in fields_dict) {
        let field = fields_dict[key];
        let value = field.get_value();
        if (value) {
          let match_type = field.df.match_type || field.df.condition || "=";
          let condition;
          if (match_type === "like") {
            condition = "like";
            if (typeof value === "string" && !value.includes("%")) {
              value = "%" + value + "%";
            }
          } else if (match_type === "=") {
            condition = "=";
            if (typeof value === "string") {
              value = value.replace(/^%+|%+$/g, "");
            }
          } else {
            condition = field.df.condition || match_type;
          }
          filters.push([
            field.df.doctype || this.list_view.doctype,
            field.df.fieldname,
            condition,
            value
          ]);
        }
      }
      return filters;
    }
    make_filter_list() {
      $(`<div class="filter-selector">
			<div class="btn-group">
				<button class="btn btn-default btn-sm filter-button">
					<span class="filter-icon button-icon">
						${frappe.utils.icon("es-line-filter")}
					</span>
					<span class="button-label hidden-xs">
					${__("Filter")}
					<span>
				</button>
				<button class="btn btn-default btn-sm filter-x-button" title="${__("Clear all filters")}">
					<span class="filter-icon button-icon">
						${frappe.utils.icon("es-small-close")}
					</span>
				</button>
			</div>
		</div>`).appendTo(this.$filter_list_wrapper);
      this.filter_button = this.$filter_list_wrapper.find(".filter-button");
      this.filter_x_button = this.$filter_list_wrapper.find(".filter-x-button");
      this.filter_list = new frappe.ui.FilterGroup({
        base_list: this.list_view,
        parent: this.$filter_list_wrapper,
        doctype: this.list_view.doctype,
        filter_button: this.filter_button,
        filter_x_button: this.filter_x_button,
        default_filters: [],
        on_change: () => this.debounced_refresh_list_view()
      });
    }
    is_being_edited() {
      return this.filter_list && this.filter_list.wrapper && this.filter_list.wrapper.find(".filter-box:visible").length > 0;
    }
  };
  frappe.views.view_modes = [
    "List",
    "Report",
    "Dashboard",
    "Gantt",
    "Kanban",
    "Calendar",
    "Image",
    "Inbox",
    "Tree",
    "Map"
  ];
  frappe.views.is_valid = (view_mode) => frappe.views.view_modes.includes(view_mode);

  // frappe/public/js/frappe/list/bulk_operations.js
  var BulkOperations = class {
    constructor({ doctype }) {
      if (!doctype)
        frappe.throw(__("Doctype required"));
      this.doctype = doctype;
    }
    print(docs) {
      const print_settings = frappe.model.get_doc(":Print Settings", "Print Settings");
      const allow_print_for_draft = cint(print_settings.allow_print_for_draft);
      const is_submittable = frappe.model.is_submittable(this.doctype);
      const allow_print_for_cancelled = cint(print_settings.allow_print_for_cancelled);
      const letterheads = this.get_letterhead_options();
      const MAX_PRINT_LIMIT = 500;
      const BACKGROUND_PRINT_THRESHOLD = 25;
      const valid_docs = docs.filter((doc) => {
        return !is_submittable || doc.docstatus === 1 || allow_print_for_cancelled && doc.docstatus == 2 || allow_print_for_draft && doc.docstatus == 0 || frappe.user.has_role("Administrator");
      }).map((doc) => doc.name);
      const invalid_docs = docs.filter((doc) => !valid_docs.includes(doc.name));
      if (invalid_docs.length > 0) {
        frappe.msgprint(__("You selected Draft or Cancelled documents"));
        return;
      }
      if (valid_docs.length === 0) {
        frappe.msgprint(__("Select atleast 1 record for printing"));
        return;
      }
      if (valid_docs.length > MAX_PRINT_LIMIT) {
        frappe.msgprint(
          __("You can only print upto {0} documents at a time", [MAX_PRINT_LIMIT])
        );
        return;
      }
      const dialog = new frappe.ui.Dialog({
        title: __("Print Documents"),
        fields: [
          {
            fieldtype: "Select",
            label: __("Letter Head"),
            fieldname: "letter_sel",
            options: letterheads,
            default: letterheads[0]
          },
          {
            fieldtype: "Select",
            label: __("Print Format"),
            fieldname: "print_sel",
            options: frappe.meta.get_print_formats(this.doctype),
            default: frappe.get_meta(this.doctype).default_print_format
          },
          {
            fieldtype: "Select",
            label: __("Page Size"),
            fieldname: "page_size",
            options: frappe.meta.get_print_sizes(),
            default: print_settings.pdf_page_size
          },
          {
            fieldtype: "Float",
            label: __("Page Height (in mm)"),
            fieldname: "page_height",
            depends_on: 'eval:doc.page_size == "Custom"',
            default: print_settings.pdf_page_height
          },
          {
            fieldtype: "Float",
            label: __("Page Width (in mm)"),
            fieldname: "page_width",
            depends_on: 'eval:doc.page_size == "Custom"',
            default: print_settings.pdf_page_width
          },
          {
            fieldtype: "Check",
            label: __("Background Print (required for >25 documents)"),
            fieldname: "background_print",
            default: valid_docs.length > BACKGROUND_PRINT_THRESHOLD,
            read_only: valid_docs.length > BACKGROUND_PRINT_THRESHOLD
          }
        ]
      });
      dialog.set_primary_action(__("Print"), (args) => {
        if (!args)
          return;
        const default_print_format = frappe.get_meta(this.doctype).default_print_format;
        const with_letterhead = args.letter_sel == __("No Letterhead") ? 0 : 1;
        const print_format = args.print_sel ? args.print_sel : default_print_format;
        const json_string = JSON.stringify(valid_docs);
        const letterhead = args.letter_sel;
        let pdf_options;
        if (args.page_size === "Custom") {
          if (args.page_height === 0 || args.page_width === 0) {
            frappe.throw(__("Page height and width cannot be zero"));
          }
          pdf_options = JSON.stringify({
            "page-height": args.page_height,
            "page-width": args.page_width
          });
        } else {
          pdf_options = JSON.stringify({ "page-size": args.page_size });
        }
        if (args.background_print) {
          frappe.call("frappe.utils.print_format.download_multi_pdf_async", {
            doctype: this.doctype,
            name: json_string,
            format: print_format,
            no_letterhead: with_letterhead ? "0" : "1",
            letterhead,
            options: pdf_options
          }).then((response) => {
            let task_id = response.message.task_id;
            frappe.realtime.task_subscribe(task_id);
            frappe.realtime.on(`task_complete:${task_id}`, (data) => {
              frappe.msgprint({
                title: __("Bulk PDF Export"),
                message: __("Your PDF is ready for download"),
                primary_action: {
                  label: __("Download PDF"),
                  client_action: "window.open",
                  args: data.file_url
                }
              });
              frappe.realtime.task_unsubscribe(task_id);
              frappe.realtime.off(`task_complete:${task_id}`);
            });
          });
        } else {
          const w = window.open(
            "/api/method/frappe.utils.print_format.download_multi_pdf?doctype=" + encodeURIComponent(this.doctype) + "&name=" + encodeURIComponent(json_string) + "&format=" + encodeURIComponent(print_format) + "&no_letterhead=" + (with_letterhead ? "0" : "1") + "&letterhead=" + encodeURIComponent(letterhead) + "&options=" + encodeURIComponent(pdf_options)
          );
          if (!w) {
            frappe.msgprint(__("Please enable pop-ups"));
          }
        }
        dialog.hide();
      });
      dialog.show();
    }
    get_letterhead_options() {
      const letterhead_options = [__("No Letterhead")];
      frappe.call({
        method: "frappe.client.get_list",
        args: {
          doctype: "Letter Head",
          fields: ["name", "is_default"],
          filters: { disabled: 0 },
          limit_page_length: 0
        },
        async: false,
        callback(r) {
          if (r.message) {
            r.message.forEach((letterhead) => {
              if (letterhead.is_default) {
                letterhead_options.unshift(letterhead.name);
              } else {
                letterhead_options.push(letterhead.name);
              }
            });
          }
        }
      });
      return letterhead_options;
    }
    delete(docnames, done = null) {
      frappe.call({
        method: "frappe.desk.reportview.delete_items",
        freeze: true,
        freeze_message: docnames.length <= 10 ? __("Deleting {0} records...", [docnames.length]) : null,
        args: {
          items: docnames,
          doctype: this.doctype
        }
      }).then((r) => {
        let failed = r.message;
        if (!failed)
          failed = [];
        if (failed.length && !r._server_messages) {
          frappe.throw(
            __("Cannot delete {0}", [failed.map((f) => f.bold()).join(", ")])
          );
        }
        if (failed.length < docnames.length) {
          frappe.utils.play_sound("delete");
          if (done)
            done();
        }
      });
    }
    assign(docnames, done) {
      if (docnames.length > 0) {
        const assign_to = new frappe.ui.form.AssignToDialog({
          obj: this,
          method: "frappe.desk.form.assign_to.add_multiple",
          doctype: this.doctype,
          docname: docnames,
          bulk_assign: true,
          re_assign: true,
          callback: done
        });
        assign_to.dialog.clear();
        assign_to.dialog.show();
      } else {
        frappe.msgprint(__("Select records for assignment"));
      }
    }
    clear_assignment(docnames, done) {
      if (docnames.length > 0) {
        frappe.call({
          method: "frappe.desk.form.assign_to.remove_multiple",
          args: {
            doctype: this.doctype,
            names: docnames,
            ignore_permissions: true
          },
          freeze: true,
          freeze_message: "Removing assignments..."
        }).then(() => {
          done();
        });
      } else {
        frappe.msgprint(__("Select records for removing assignment"));
      }
    }
    apply_assignment_rule(docnames, done) {
      if (docnames.length > 0) {
        frappe.call("frappe.automation.doctype.assignment_rule.assignment_rule.bulk_apply", {
          doctype: this.doctype,
          docnames
        }).then(() => done());
      }
    }
    submit_or_cancel(docnames, action = "submit", done = null) {
      action = action.toLowerCase();
      const task_id = Math.random().toString(36).slice(-5);
      frappe.realtime.task_subscribe(task_id);
      return frappe.xcall("frappe.desk.doctype.bulk_update.bulk_update.submit_cancel_or_update_docs", {
        doctype: this.doctype,
        action,
        docnames,
        task_id
      }).then((failed_docnames) => {
        if (failed_docnames == null ? void 0 : failed_docnames.length) {
          const comma_separated_records = frappe.utils.comma_and(failed_docnames);
          switch (action) {
            case "submit":
              frappe.throw(__("Cannot submit {0}.", [comma_separated_records]));
              break;
            case "cancel":
              frappe.throw(__("Cannot cancel {0}.", [comma_separated_records]));
              break;
            default:
              frappe.throw(__("Cannot {0} {1}.", [action, comma_separated_records]));
          }
        }
        if ((failed_docnames == null ? void 0 : failed_docnames.length) < docnames.length) {
          frappe.utils.play_sound(action);
          if (done)
            done();
        }
      }).finally(() => {
        frappe.realtime.task_unsubscribe(task_id);
      });
    }
    edit(docnames, field_mappings, done) {
      const field_options = Object.keys(field_mappings).sort(function(a, b) {
        return __(cstr(field_mappings[a].label)).localeCompare(
          cstr(__(field_mappings[b].label))
        );
      });
      const field_autocomplete_options = field_options.map((key) => ({
        label: __(cstr(key)),
        value: key
      }));
      const status_regex = /status/i;
      const default_field = field_options.find((value) => status_regex.test(value)) || field_options.find((value) => {
        var _a3;
        return ((_a3 = field_mappings[value]) == null ? void 0 : _a3.fieldtype) === "Select";
      });
      const dialog = new frappe.ui.Dialog({
        title: __("Bulk Edit"),
        fields: [
          {
            fieldtype: "Autocomplete",
            options: field_autocomplete_options,
            max_items: Infinity,
            default: default_field,
            label: __("Field"),
            fieldname: "field",
            reqd: 1,
            onchange: () => {
              set_value_field(dialog);
            }
          },
          {
            fieldtype: "Data",
            label: __("Value"),
            fieldname: "value",
            onchange() {
              show_help_text();
            }
          }
        ],
        primary_action: ({ value }) => {
          const selected_field = field_mappings[dialog.get_value("field")];
          const { fieldname, is_child_field, child_doctype } = selected_field;
          dialog.disable_primary_action();
          let update_data = {};
          if (is_child_field) {
            update_data = {
              child_table_updates: {
                [child_doctype]: {
                  [fieldname]: value || null
                }
              }
            };
          } else {
            update_data[fieldname] = value || null;
          }
          frappe.call({
            method: "frappe.desk.doctype.bulk_update.bulk_update.submit_cancel_or_update_docs",
            args: {
              doctype: this.doctype,
              freeze: true,
              docnames,
              action: "update",
              data: update_data
            }
          }).then((r) => {
            let failed = r.message || [];
            if (failed.length && !r._server_messages) {
              dialog.enable_primary_action();
              frappe.throw(
                __("Cannot update {0}", [
                  failed.map((f) => f.bold ? f.bold() : f).join(", ")
                ])
              );
            }
            done();
            dialog.hide();
            frappe.show_alert(__("Updated successfully"));
          });
        },
        primary_action_label: __("Update {0} records", [docnames.length])
      });
      if (default_field)
        set_value_field(dialog);
      show_help_text();
      function set_value_field(dialogObj) {
        var _a3;
        const field_value = dialogObj.get_value("field");
        if (!field_value || !field_mappings[field_value])
          return;
        const new_df = Object.assign({}, field_mappings[field_value]);
        if (((_a3 = new_df.label) == null ? void 0 : _a3.match(status_regex)) && new_df.fieldtype === "Select" && !new_df.default) {
          let options = [];
          if (typeof new_df.options === "string") {
            options = new_df.options.split("\n");
          }
          new_df.default = options[0] || options[1];
        }
        new_df.label = __("Value");
        new_df.onchange = show_help_text;
        delete new_df.depends_on;
        delete new_df.is_child_field;
        delete new_df.child_doctype;
        dialogObj.replace_field("value", new_df);
        show_help_text();
      }
      function show_help_text() {
        if (dialog.get_primary_btn().is(":focus, :active"))
          return;
        let value = dialog.get_value("value");
        if (value == null || value === "") {
          dialog.set_df_property(
            "value",
            "description",
            __("You have not entered a value. The field will be set to empty.")
          );
        } else {
          dialog.set_df_property("value", "description", "");
        }
      }
      dialog.refresh();
      dialog.show();
    }
    add_tags(docnames, done) {
      const dialog = new frappe.ui.Dialog({
        title: __("Add Tags"),
        fields: [
          {
            fieldtype: "MultiSelectPills",
            fieldname: "tags",
            label: __("Tags"),
            reqd: true,
            get_data: function(txt) {
              return frappe.db.get_link_options("Tag", txt);
            }
          }
        ],
        primary_action_label: __("Add"),
        primary_action: () => {
          let args = dialog.get_values();
          if (args && args.tags) {
            dialog.set_message("Adding Tags...");
            frappe.call({
              method: "frappe.desk.doctype.tag.tag.add_tags",
              args: {
                tags: args.tags,
                dt: this.doctype,
                docs: docnames
              },
              callback: () => {
                dialog.hide();
                done();
              }
            });
          }
        }
      });
      dialog.show();
    }
    export(doctype, docnames) {
      frappe.require("data_import_tools.bundle.js", () => {
        const data_exporter = new frappe.data_import.DataExporter(
          doctype,
          "Insert New Records"
        );
        data_exporter.dialog.set_value("export_records", "by_filter");
        data_exporter.filter_group.add_filters_to_filter_group([
          [doctype, "name", "in", docnames, false]
        ]);
      });
    }
  };
  frappe.ui.BulkOperations = BulkOperations;

  // frappe/public/js/frappe/list/list_settings.js
  var ListSettings = class {
    constructor({ listview, doctype, meta, settings }) {
      if (!doctype) {
        frappe.throw("DocType required");
      }
      this.listview = listview;
      this.doctype = doctype;
      this.meta = meta;
      this.settings = settings;
      this.dialog = null;
      this.fields = this.settings && this.settings.fields ? JSON.parse(this.settings.fields) : [];
      this.subject_field = null;
      this.max_number_of_fields = 50;
      frappe.model.with_doctype("List View Settings", () => {
        this.make();
        this.get_listview_fields(meta);
        this.setup_fields();
        this.setup_remove_fields();
        this.add_new_fields();
        this.show_dialog();
      });
    }
    make() {
      let me2 = this;
      let list_view_settings = frappe.get_meta("List View Settings");
      me2.dialog = new frappe.ui.Dialog({
        title: __("{0} List View Settings", [__(me2.doctype)]),
        fields: list_view_settings.fields
      });
      me2.dialog.set_values(me2.settings);
      me2.dialog.set_primary_action(__("Save"), () => {
        let values = me2.dialog.get_values();
        frappe.show_alert({
          message: __("Saving"),
          indicator: "green"
        });
        frappe.call({
          method: "frappe.desk.doctype.list_view_settings.list_view_settings.save_listview_settings",
          args: {
            doctype: me2.doctype,
            listview_settings: values,
            removed_listview_fields: me2.removed_fields || []
          },
          callback: function(r) {
            me2.listview.refresh_columns(r.message.meta, r.message.listview_settings);
            me2.dialog.hide();
          }
        });
      });
    }
    refresh() {
      let me2 = this;
      me2.setup_fields();
      me2.add_new_fields();
      me2.setup_remove_fields();
    }
    show_dialog() {
      let me2 = this;
      if (!this.settings.fields) {
        me2.update_fields();
      }
      if (!me2.dialog.get_value("total_fields")) {
        let field_count = this.settings.total_fields;
        if (!field_count) {
          field_count = me2.fields.length;
          if (field_count < 4) {
            field_count = 4;
          } else if (field_count > 10) {
            field_count = 10;
          }
        }
        me2.dialog.set_value("total_fields", field_count);
      }
      me2.dialog.show();
    }
    setup_fields() {
      function is_status_field(field) {
        return field.fieldname === "status_field";
      }
      let me2 = this;
      let fields_html = me2.dialog.get_field("fields_html");
      let wrapper = fields_html.$wrapper[0];
      let fields = ``;
      for (let idx in me2.fields) {
        if (idx == parseInt(this.max_number_of_fields)) {
          break;
        }
        let is_sortable = idx == 0 ? `` : `sortable`;
        let show_sortable_handle = idx == 0 ? `hide` : ``;
        let can_remove = idx == 0 || is_status_field(me2.fields[idx]) ? `hide` : `d-flex`;
        fields += `
				<div class="control-input form-control fields_order ${is_sortable} flex"
	 				style="margin-bottom: 5px; padding-bottom: 1.5px;"
	 				data-fieldname="${me2.fields[idx].fieldname}"
	 				data-label="${me2.fields[idx].label}"
	 				data-type="${me2.fields[idx].type}">

					<div class="row flex-fill align-items-center">
						<div class="col-1 d-flex align-items-center justify-content-center px-1">
							${frappe.utils.icon("drag", "xs", "", "", "sortable-handle " + show_sortable_handle)}
						</div>

						<div class="col d-flex align-items-center px-0">
							${__(me2.fields[idx].label, null, me2.doctype)}
						</div>

						<div class="col-1 d-flex align-items-center justify-content-center px-0">
							<a class="text-muted remove-field align-items-center ${can_remove}"
							   data-fieldname="${me2.fields[idx].fieldname}">
								${frappe.utils.icon("x", "xs")}
							</a>
						</div>
					</div>
				</div>`;
      }
      fields_html.html(`
			<div class="form-group">
				<div class="clearfix">
					<label class="control-label" style="padding-right: 0px;">${__("Fields")}</label>
					<label class="text-extra-muted float-right">
						<a class="add-new-fields text-muted">
							${__("+ Add / Remove Fields")}
						</a>
					</label>
				</div>
				<div class="control-input-wrapper">
				${fields}
				</div>
			</div>
		`);
      new Sortable(wrapper.getElementsByClassName("control-input-wrapper")[0], {
        handle: ".sortable-handle",
        draggable: ".sortable",
        onUpdate: () => {
          me2.update_fields();
          me2.refresh();
        }
      });
    }
    add_new_fields() {
      let me2 = this;
      let fields_html = me2.dialog.get_field("fields_html");
      let add_new_fields = fields_html.$wrapper[0].getElementsByClassName("add-new-fields")[0];
      add_new_fields.onclick = () => me2.column_selector();
    }
    setup_remove_fields() {
      let me2 = this;
      let fields_html = me2.dialog.get_field("fields_html");
      let remove_fields = fields_html.$wrapper[0].getElementsByClassName("remove-field");
      for (let idx = 0; idx < remove_fields.length; idx++) {
        remove_fields.item(idx).onclick = () => me2.remove_fields(remove_fields.item(idx).getAttribute("data-fieldname"));
      }
    }
    remove_fields(fieldname) {
      let me2 = this;
      let existing_fields = me2.fields.map((f) => f.fieldname);
      for (let idx in me2.fields) {
        let field = me2.fields[idx];
        if (field.fieldname == fieldname) {
          me2.fields.splice(idx, 1);
          break;
        }
      }
      me2.set_removed_fields(
        me2.get_removed_listview_fields(
          me2.fields.map((f) => f.fieldname),
          existing_fields
        )
      );
      me2.refresh();
      me2.update_fields();
    }
    update_fields() {
      let me2 = this;
      let fields_html = me2.dialog.get_field("fields_html");
      let wrapper = fields_html.$wrapper[0];
      let fields_order = wrapper.getElementsByClassName("fields_order");
      me2.fields = [];
      for (let idx = 0; idx < fields_order.length; idx++) {
        me2.fields.push({
          fieldname: fields_order.item(idx).getAttribute("data-fieldname"),
          label: __(fields_order.item(idx).getAttribute("data-label"))
        });
      }
      me2.dialog.set_value("fields", JSON.stringify(me2.fields));
      me2.dialog.get_value("fields");
    }
    column_selector() {
      let me2 = this;
      let d = new frappe.ui.Dialog({
        title: __("{0} Fields", [__(me2.doctype)]),
        fields: [
          {
            label: __("Reset Fields"),
            fieldtype: "Button",
            fieldname: "reset_fields",
            click: () => me2.reset_listview_fields(d)
          },
          {
            label: __("Select Fields (Up to {0})", [this.max_number_of_fields]),
            fieldtype: "MultiCheck",
            fieldname: "fields",
            options: me2.get_doctype_fields(
              me2.meta,
              me2.fields.map((f) => f.fieldname)
            ),
            columns: 2
          }
        ]
      });
      d.set_primary_action(__("Save"), () => {
        let values = d.get_values().fields;
        me2.set_removed_fields(
          me2.get_removed_listview_fields(
            values,
            me2.fields.map((f) => f.fieldname)
          )
        );
        me2.fields = [];
        me2.set_subject_field(me2.meta);
        me2.set_status_field();
        for (let idx in values) {
          let value = values[idx];
          if (me2.fields.length === parseInt(this.max_number_of_fields)) {
            break;
          } else if (value != me2.subject_field.fieldname) {
            let field = frappe.meta.get_docfield(me2.doctype, value);
            if (field) {
              me2.fields.push({
                label: __(field.label, null, me2.doctype),
                fieldname: field.fieldname
              });
            }
          }
        }
        me2.refresh();
        me2.dialog.set_value("fields", JSON.stringify(me2.fields));
        d.hide();
      });
      d.show();
    }
    reset_listview_fields(dialog) {
      let me2 = this;
      frappe.xcall(
        "frappe.desk.doctype.list_view_settings.list_view_settings.get_default_listview_fields",
        {
          doctype: me2.doctype
        }
      ).then((fields) => {
        let field = dialog.get_field("fields");
        field.df.options = me2.get_doctype_fields(me2.meta, fields);
        dialog.refresh();
      });
    }
    get_listview_fields(meta) {
      let me2 = this;
      if (!me2.settings.fields) {
        me2.set_list_view_fields(meta);
      } else {
        me2.fields = JSON.parse(this.settings.fields);
      }
      me2.fields.uniqBy((f) => f.fieldname);
    }
    set_list_view_fields(meta) {
      let me2 = this;
      me2.set_subject_field(meta);
      me2.set_status_field();
      meta.fields.forEach((field) => {
        if (field.in_list_view && !frappe.model.no_value_type.includes(field.fieldtype) && me2.subject_field.fieldname != field.fieldname) {
          me2.fields.push({
            label: __(field.label, null, me2.doctype),
            fieldname: field.fieldname
          });
        }
      });
    }
    set_subject_field(meta) {
      let me2 = this;
      me2.subject_field = {
        label: __("ID"),
        fieldname: "name"
      };
      if (meta.title_field) {
        let field = frappe.meta.get_docfield(me2.doctype, meta.title_field.trim());
        me2.subject_field = {
          label: __(field.label, null, me2.doctype),
          fieldname: field.fieldname
        };
      }
      me2.fields.push(me2.subject_field);
    }
    set_status_field() {
      let me2 = this;
      if (frappe.has_indicator(me2.doctype)) {
        me2.fields.push({
          type: "Status",
          label: __("Status"),
          fieldname: "status_field"
        });
      }
    }
    get_doctype_fields(meta, fields) {
      let multiselect_fields = [];
      meta.fields.forEach((field) => {
        if (!frappe.model.no_value_type.includes(field.fieldtype)) {
          multiselect_fields.push({
            label: __(field.label, null, field.doctype),
            value: field.fieldname,
            checked: fields.includes(field.fieldname)
          });
        }
      });
      return multiselect_fields;
    }
    get_removed_listview_fields(new_fields, existing_fields) {
      let me2 = this;
      let removed_fields = [];
      if (frappe.has_indicator(me2.doctype)) {
        new_fields.push("status_field");
      }
      existing_fields.forEach((column) => {
        if (!new_fields.includes(column)) {
          removed_fields.push(column);
        }
      });
      return removed_fields;
    }
    set_removed_fields(fields) {
      let me2 = this;
      if (me2.removed_fields) {
        me2.removed_fields = me2.removed_fields.concat(fields);
      } else {
        me2.removed_fields = fields;
      }
    }
  };

  // frappe/public/js/frappe/list/list_view.js
  frappe.provide("frappe.views");
  frappe.views.ListView = class ListView extends frappe.views.BaseList {
    static load_last_view() {
      const route = frappe.get_route();
      const doctype = route[1];
      if (route.length === 2) {
        const user_settings = frappe.get_user_settings(doctype);
        const last_view = user_settings.last_view;
        frappe.set_route(
          "list",
          frappe.router.doctype_layout || doctype,
          frappe.views.is_valid(last_view) ? last_view.toLowerCase() : "list"
        );
        return true;
      }
      return false;
    }
    constructor(opts) {
      super(opts);
      this.show();
      const meta = frappe.get_meta(this.doctype);
      this.is_large_table = meta == null ? void 0 : meta.is_large_table;
      this.debounced_refresh = frappe.utils.debounce(
        this.process_document_refreshes.bind(this),
        this.is_large_table ? 15e3 : 2e3
      );
      this.count_upper_bound = 1001;
      this._element_factory = new ElementFactory(this.doctype);
      this.column_max_widths = {};
      this.max_number_of_avatars = 3;
      this.max_number_of_fields = 50;
    }
    has_permissions() {
      return frappe.perm.has_perm(this.doctype, 0, "read");
    }
    show() {
      this.parent.disable_scroll_to_top = true;
      super.show();
    }
    check_permissions() {
      if (!this.has_permissions()) {
        frappe.set_route("");
        frappe.throw(__("Not permitted to view {0}", [this.doctype]));
      }
    }
    show_skeleton() {
      this.$list_skeleton = this.parent.page.container.find(".list-skeleton");
      if (!this.$list_skeleton.length) {
        this.$list_skeleton = $(`
				<div class="row list-skeleton">
					<div class="col-lg-2">
						<div class="list-skeleton-box"></div>
					</div>
					<div class="col">
						<div class="list-skeleton-box"></div>
					</div>
				</div>
			`);
        this.parent.page.container.find(".page-content").append(this.$list_skeleton);
      }
      this.parent.page.container.find(".layout-main").hide();
      this.$list_skeleton.show();
    }
    hide_skeleton() {
      this.$list_skeleton && this.$list_skeleton.hide();
      this.parent.page.container.find(".layout-main").show();
    }
    get view_name() {
      return "List";
    }
    get view_user_settings() {
      return this.user_settings[this.view_name] || {};
    }
    setup_defaults() {
      super.setup_defaults();
      this.view = "List";
      this.sort_by = this.view_user_settings.sort_by || this.sort_by || "creation";
      this.sort_order = this.view_user_settings.sort_order || this.sort_order || "desc";
      this.menu_items = this.menu_items.concat(this.get_menu_items());
      if (Array.isArray(this.view_user_settings.filters)) {
        const saved_filters = this.view_user_settings.filters;
        this.filters = this.validate_filters(saved_filters);
      } else {
        this.filters = (this.settings.filters || []).map((f) => {
          if (f.length === 3) {
            f = [this.doctype, f[0], f[1], f[2]];
          }
          return f;
        });
      }
      this.patch_refresh_and_load_lib();
      return this.get_list_view_settings().then(() => this.add_recent_filter_on_large_tables());
    }
    add_recent_filter_on_large_tables() {
      var _a3;
      if (!this.is_large_table || ((_a3 = this.list_view_settings) == null ? void 0 : _a3.disable_automatic_recency_filters)) {
        return;
      }
      const recency_field = "creation";
      if (this.filters.length) {
        return;
      }
      this.filters.push([this.doctype, recency_field, "Timespan", "last 90 days"]);
      frappe.show_alert(
        {
          message: __(
            "Automatically applied a filter for recent data. You can disable this behavior from the list view settings."
          ),
          indicator: "yellow"
        },
        3
      );
    }
    on_sort_change(sort_by, sort_order) {
      this.sort_by = sort_by;
      this.sort_order = sort_order;
      super.on_sort_change();
    }
    validate_filters(filters) {
      let valid_fields = this.meta.fields.map((df) => df.fieldname);
      valid_fields = valid_fields.concat(frappe.model.std_fields_list);
      return filters.filter((f) => valid_fields.includes(f[1])).uniqBy((f) => f[1]);
    }
    setup_page() {
      this.parent.list_view = this;
      super.setup_page();
    }
    setup_page_head() {
      super.setup_page_head();
      this.set_primary_action();
      this.set_actions_menu_items();
    }
    set_actions_menu_items() {
      this.actions_menu_items = this.get_actions_menu_items();
      this.workflow_action_menu_items = this.get_workflow_action_menu_items();
      this.workflow_action_items = {};
      const actions = this.actions_menu_items.concat(this.workflow_action_menu_items);
      actions.forEach((item) => {
        const $item = this.page.add_actions_menu_item(item.label, item.action, item.standard);
        if (item.class) {
          $item.addClass(item.class);
        }
        if (item.is_workflow_action && $item) {
          this.workflow_action_items[item.name] = $item;
        }
      });
    }
    show_restricted_list_indicator_if_applicable() {
      const match_rules_list = frappe.perm.get_match_rules(this.doctype);
      if (match_rules_list.length) {
        this.restricted_list = $(
          `<button class="btn btn-xs restricted-button flex align-center">
					${frappe.utils.icon("restriction", "xs")}
				</button>`
        ).click(() => this.show_restrictions(match_rules_list)).appendTo(this.page.page_form.find(".filter-section"));
      }
    }
    show_restrictions(match_rules_list = []) {
      frappe.msgprint(
        frappe.render_template("list_view_permission_restrictions", {
          condition_list: match_rules_list
        }),
        __("Restrictions", null, "Title of message showing restrictions in list view")
      );
    }
    get_fields() {
      return super.get_fields().concat(
        Object.entries(this.link_field_title_fields || {}).map(
          (entry) => entry.join(".") + " as " + entry.join("_")
        )
      );
    }
    async set_fields() {
      this.link_field_title_fields = {};
      let fields = [].concat(
        frappe.model.std_fields_list,
        this.get_fields_in_list_view(),
        [this.meta.title_field, this.meta.image_field],
        this.settings.add_fields || [],
        this.meta.track_seen ? "_seen" : null,
        this.sort_by,
        "enabled",
        "disabled",
        "color"
      );
      await Promise.all(
        fields.map((f) => {
          return new Promise((resolve) => {
            const df = typeof f === "string" ? frappe.meta.get_docfield(this.doctype, f) : f;
            if (df && df.fieldtype == "Link" && frappe.boot.link_title_doctypes.includes(df.options)) {
              frappe.model.with_doctype(df.options, () => {
                const meta = frappe.get_meta(df.options);
                if (meta.show_title_field_in_link && meta.title_field) {
                  this.link_field_title_fields[typeof f === "string" ? f : f.fieldname] = meta.title_field;
                }
                this._add_field(f);
                resolve();
              });
            } else {
              this._add_field(f);
              resolve();
            }
          });
        })
      );
      this.fields.forEach((f) => {
        const df = frappe.meta.get_docfield(f[1], f[0]);
        if (df && df.fieldtype === "Currency" && df.options && !df.options.includes(":")) {
          this._add_field(df.options);
        }
      });
    }
    patch_refresh_and_load_lib() {
      this.refresh = this.refresh.bind(this);
      this.refresh = frappe.utils.throttle(this.refresh, 1e3);
      this.load_lib = new Promise((resolve) => {
        if (this.required_libs) {
          frappe.require(this.required_libs, resolve);
        } else {
          resolve();
        }
      });
      const interval = 5 * 60 * 1e3;
      setInterval(() => {
        if (frappe.get_route_str() === this.page_name) {
          this.refresh();
        }
      }, interval);
    }
    set_primary_action() {
      if (this.can_create && !frappe.boot.read_only) {
        const doctype_name = __(frappe.router.doctype_layout) || __(this.doctype);
        const add_button_label = __("Add {0}", [doctype_name], "Primary action in list view");
        const create_button = this.page.set_primary_action(
          add_button_label,
          () => {
            if (this.settings.primary_action) {
              this.settings.primary_action();
            } else {
              this.make_new_doc();
            }
          },
          "add"
        );
        frappe.ui.keys.add_shortcut({
          shortcut: "ctrl+b",
          action: () => {
            if (this.settings.primary_action) {
              this.settings.primary_action();
            } else {
              this.make_new_doc();
            }
            return true;
          },
          description: __(
            "Create a new document",
            null,
            "Description of a list view shortcut"
          ),
          page: this.page
        });
        if (frappe.is_mobile()) {
          create_button.append(__("Add"));
        } else {
          this._trim_primary_action_if_overflow(create_button, add_button_label);
        }
      } else {
        frappe.ui.keys.off("ctrl+b", this.page);
        this.page.clear_primary_action();
      }
    }
    _trim_primary_action_if_overflow(btn, add_button_label) {
      const container = this.page.wrapper.find(".page-head-content")[0];
      if (!container || !btn[0])
        return;
      const containerRect = container.getBoundingClientRect();
      const btnRect = btn[0].getBoundingClientRect();
      if (btnRect.right > containerRect.right) {
        const short_label = __("Add");
        btn.attr("title", add_button_label).tooltip();
        btn.find("span").text(short_label);
      }
    }
    make_new_doc() {
      const doctype = this.doctype;
      const options = {};
      const allowed_filter_types = [
        "=",
        "descendants of (inclusive)",
        "descendants of",
        "ancestors of"
      ];
      this.filter_area.get().forEach((f) => {
        if (allowed_filter_types.includes(f[2]) && frappe.model.is_non_std_field(f[1])) {
          const df = frappe.meta.get_field(doctype, f[1]);
          if (df && !df.read_only) {
            options[f[1]] = f[3];
          }
        }
      });
      frappe.new_doc(doctype, options);
    }
    setup_view() {
      this.setup_columns();
      this.render_header();
      this.render_skeleton();
      this.setup_events();
      this.settings.onload && this.settings.onload(this);
      this.show_restricted_list_indicator_if_applicable();
    }
    refresh_columns(meta, list_view_settings) {
      this.meta = meta;
      this.tags_shown = list_view_settings == null ? void 0 : list_view_settings.show_tags;
      this.list_view_settings = list_view_settings;
      this.setup_columns();
      this.refresh();
    }
    refresh(refresh_header = false) {
      return super.refresh().then(() => {
        this.render_header(refresh_header);
        this.render_count();
        this.update_checkbox();
        this.update_url_with_filters();
        this.setup_realtime_updates();
        this.apply_styles_basedon_dropdown();
      });
    }
    update_checkbox(target) {
      if (!this.$checkbox_actions)
        return;
      let $check_all_checkbox = this.$checkbox_actions.find(".list-check-all");
      if ($check_all_checkbox.prop("checked") && target && !target.prop("checked")) {
        $check_all_checkbox.prop("checked", false);
      }
      $check_all_checkbox.prop("checked", this.$checks.length === this.data.length);
    }
    setup_freeze_area() {
      this.$freeze = $(
        `<div class="freeze flex justify-center align-center text-muted">
				${__("Loading")}...
			</div>`
      ).hide();
      this.$result.append(this.$freeze);
    }
    setup_columns() {
      this.columns = [];
      const get_df = frappe.meta.get_docfield.bind(null, this.doctype);
      if (this.meta.title_field) {
        this.columns.push({
          type: "Subject",
          df: get_df(this.meta.title_field)
        });
      } else {
        this.columns.push({
          type: "Subject",
          df: {
            label: __("ID"),
            fieldname: "name"
          }
        });
      }
      if (frappe.has_indicator(this.doctype)) {
        this.columns.push({
          type: "Status"
        });
      }
      const fields_in_list_view = this.get_fields_in_list_view();
      this.columns = this.columns.concat(
        fields_in_list_view.filter((df) => {
          if (frappe.has_indicator(this.doctype) && df.fieldname === "status") {
            return false;
          }
          if (!df.in_list_view || df.is_virtual) {
            return false;
          }
          return df.fieldname !== this.meta.title_field;
        }).map((df) => ({
          type: "Field",
          df
        }))
      );
      if (this.list_view_settings.fields) {
        this.columns = this.reorder_listview_fields();
      }
      let total_fields = 6;
      if (window.innerWidth <= 1366) {
        total_fields = 4;
      } else if (window.innerWidth >= 1920) {
        total_fields = 10;
      }
      this.columns = this.columns.slice(0, this.max_number_of_fields);
      this.columns.splice(1, 0, {
        type: "Tag"
      });
      if (!this.settings.hide_name_column && this.meta.title_field && this.meta.title_field !== "name") {
        this.columns.push({
          type: "Field",
          df: {
            label: __("ID"),
            fieldname: "name"
          }
        });
      }
    }
    reorder_listview_fields() {
      let fields_order = [];
      let fields = JSON.parse(this.list_view_settings.fields);
      fields_order.push(this.columns[0]);
      this.columns.splice(0, 1);
      for (let fld in fields) {
        for (let col in this.columns) {
          let field = fields[fld];
          let column = this.columns[col];
          if (column.type == "Status" && field.fieldname == "status_field") {
            fields_order.push(column);
            break;
          } else if (column.type == "Field" && field.fieldname === column.df.fieldname) {
            fields_order.push(column);
            break;
          }
        }
      }
      return fields_order;
    }
    get_documentation_link() {
      if (this.meta.documentation) {
        return `<a href="${this.meta.documentation}" target="blank" class="meta-description small text-muted">${__("Need Help?")}</a>`;
      }
      return "";
    }
    get_no_result_message() {
      let help_link = this.get_documentation_link();
      let filters = this.filter_area && this.filter_area.get();
      let has_filters_set = filters && filters.length;
      let no_result_message = has_filters_set ? __("No {0} found with matching filters. Clear filters to see all {0}.", [
        __(this.doctype)
      ]) : this.meta.description ? __(this.meta.description) : __("You haven't created a {0} yet", [__(this.doctype)]);
      let new_button_label = has_filters_set ? __("Create a new {0}", [__(this.doctype)], "Create a new document from list view") : __(
        "Create your first {0}",
        [__(this.doctype)],
        "Create a new document from list view"
      );
      const new_button = this.can_create ? `<p><button class="btn btn-default btn-sm btn-new-doc hidden-xs">
				${new_button_label}
			</button> <button class="btn btn-primary btn-new-doc visible-xs">
				${__("Create New", null, "Create a new document from list view")}
			</button></p>` : "";
      return `<div class="msg-box no-border">
			<div class="mb-4">
			  	<svg class="icon icon-xl" style="stroke: var(--text-light);">
					<use href="#icon-small-file"></use>
				</svg>
			</div>
			<p>${no_result_message}</p>
			${new_button}
			${help_link}
		</div>`;
    }
    freeze() {
      if (this.list_view_settings && !this.list_view_settings.disable_count) {
        this.get_count_element().html(
          `<span>${__("Refreshing", null, "Document count in list view")}...</span>`
        );
      }
    }
    get_args() {
      const args = super.get_args();
      if (this.list_view_settings && !this.list_view_settings.disable_comment_count) {
        args.with_comment_count = 1;
      } else {
        args.with_comment_count = 0;
      }
      return args;
    }
    before_refresh() {
      if (frappe.route_options && this.filter_area) {
        this.filters = this.parse_filters_from_route_options();
        frappe.route_options = null;
        if (this.filters.length > 0) {
          return this.filter_area.clear(false).then(() => this.filter_area.set(this.filters));
        }
      }
      return Promise.resolve();
    }
    parse_filters_from_settings() {
      return (this.settings.filters || []).map((f) => {
        if (f.length === 3) {
          f = [this.doctype, f[0], f[1], f[2]];
        }
        return f;
      });
    }
    toggle_result_area() {
      super.toggle_result_area();
      this.toggle_actions_menu_button(
        this.$result.find(".list-row-checkbox:checked").length > 0
      );
    }
    toggle_actions_menu_button(toggle) {
      if (toggle) {
        this.page.show_actions_menu();
        this.page.clear_primary_action();
      } else {
        this.page.hide_actions_menu();
        this.set_primary_action();
      }
    }
    render_header(refresh_header = false) {
      if (refresh_header) {
        this.$result.find(".list-row-head").remove();
      }
      if (this.$result.find(".list-row-head").length === 0) {
        this.$result.prepend(this.get_header_html());
        if (this.filter_area.filter_list.get_filter_value("_liked_by")) {
          this.$result.find(".list-liked-by-me").addClass("liked");
        }
      }
    }
    render_skeleton() {
      const $row = this.get_list_row_html_skeleton(
        '<div><input type="checkbox" class="render-list-checkbox"/></div>'
      );
      this.$result.append($row);
    }
    before_render() {
      this.settings.before_render && this.settings.before_render();
      frappe.model.user_settings.save(this.doctype, "last_view", this.view_name);
      this.save_view_user_settings({
        filters: this.filter_area && this.filter_area.get(),
        sort_by: this.sort_selector && this.sort_selector.sort_by,
        sort_order: this.sort_selector && this.sort_selector.sort_order
      });
    }
    after_render() {
      this.$no_result.html(this.get_no_result_message());
      this.setup_new_doc_event();
    }
    render() {
      this.render_list();
      this.set_rows_as_checked();
    }
    render_list() {
      var _a3;
      this.$result.find(".list-row-container").remove();
      this.parent.page.main.parent().addClass("list-view");
      this.render_header();
      let has_assignto = false;
      let assign_to_count = 0;
      let assign_to_length = 0;
      if (this.data.length > 0) {
        let idx = 0;
        for (let doc of this.data) {
          doc._idx = idx++;
          this.$result.append(this.get_list_row_html(doc));
          if (doc._assign) {
            assign_to_length = (_a3 = JSON.parse(doc._assign)) == null ? void 0 : _a3.length;
            assign_to_count = Math.max(
              assign_to_count,
              assign_to_length > this.max_number_of_avatars ? this.max_number_of_avatars : assign_to_length
            );
            if (!has_assignto) {
              has_assignto = true;
            }
          }
        }
      }
      this.apply_column_widths();
      this.update_listview_classes(has_assignto, assign_to_count);
    }
    render_count() {
      var _a3;
      if ((_a3 = this.list_view_settings) == null ? void 0 : _a3.disable_count) {
        return;
      }
      let me2 = this;
      let $count = this.get_count_element();
      $count.css("white-space", "nowrap");
      this.get_count_str().then((count) => {
        $count.html(`<span>${count}</span>`);
        if (this.count_upper_bound && (this.total_count == this.count_upper_bound || this.total_count == null)) {
          $count.attr(
            "title",
            __(
              "The count shown is an estimated count. Click here to see the accurate count."
            )
          );
          $count.tooltip({ delay: { show: 600, hide: 100 }, trigger: "hover" });
          $count.css("cursor", "pointer");
          $count.on("click", () => {
            me2.count_upper_bound = 0;
            $count.off("click");
            $count.tooltip("disable");
            me2.freeze();
            me2.render_count();
            $count.css("cursor", "");
          });
        }
      });
    }
    get_count_element() {
      var _a3;
      return (_a3 = this.$result) == null ? void 0 : _a3.find(".list-count");
    }
    get_header_html() {
      if (!this.columns) {
        return;
      }
      const subject_field = this.columns[0].df;
      let subject_html = `
			<span class="level-item select-like">
				<input class="list-header-checkbox list-check-all" type="checkbox" title="${__("Select All")}">
			</span>
			<span class="level-item" data-sort-by="${subject_field.fieldname}"
				title="${__("Click to sort by {0}", [subject_field.label])}">
				${__(subject_field.label)}
			</span>
		`;
      let $columns = this.columns.map((col) => {
        var _a3, _b, _c, _d;
        let classes = [
          "list-row-col ellipsis",
          col.type == "Subject" ? "list-subject level" : "hidden-xs",
          col.type == "Tag" ? `tag-col ${!this.tags_shown ? "hide" : ""} ` : "",
          frappe.model.is_numeric_field(col.df) ? "text-right" : "",
          (_a3 = col.df) == null ? void 0 : _a3.fieldname
        ].join(" ");
        let html = "";
        if (col.type === "Subject") {
          html = subject_html;
        } else {
          const fieldname = (_b = col.df) == null ? void 0 : _b.fieldname;
          const label = __(((_c = col.df) == null ? void 0 : _c.label) || col.type, null, (_d = col.df) == null ? void 0 : _d.parent);
          const title = __("Click to sort by {0}", [label]);
          const attrs = fieldname ? `data-sort-by="${fieldname}" title="${title}"` : "";
          html = `<span ${attrs}>${label}</span>`;
        }
        return `<div class="${classes}">${html}</div>
			`;
      }).join("");
      if (this.settings.button) {
        $columns += `<div class="list-row-col hidden-xs"></div>`;
      }
      if (this.settings.dropdown_button) {
        $columns += `<div class="list-row-col hidden-xs"></div>`;
      }
      const right_html = `
			<span class="list-count" style=""></span>
			<span class="level-item list-liked-by-me hidden-xs">
				<span title="${__("Liked by me")}">
					<svg class="icon icon-sm like-icon">
						<use href="#icon-heart"></use>
					</svg>
				</span>
			</span>
		`;
      return this.get_header_html_skeleton($columns, right_html);
    }
    get_header_html_skeleton(left2 = "", right2 = "") {
      return `
		<div class="list-row-container">
			<header class="level list-row-head text-muted">
				<div class="level-left list-header-subject">
					${left2}
				</div>
				<div class="level-left checkbox-actions">
					<div class="level list-subject">
						<span class="level-item select-like">
							<input class="list-header-checkbox list-check-all" type="checkbox" title="${__("Select All")}">
						</span>
						<span class="level-item list-header-meta"></span>
					</div>
				</div>
				<div class="level-right">
					${right2}
				</div>
			</header>
		</div>
		`;
    }
    get_left_html(doc) {
      var _a3;
      let left_html = "";
      const mobile_field_columns = this.columns.filter(
        (col) => {
          var _a4;
          return col.type === "Field" && ((_a4 = col.df) == null ? void 0 : _a4.fieldname);
        }
      );
      let has_value_in_second_column = true;
      if (mobile_field_columns.length > 1) {
        const fieldname = mobile_field_columns[1].df.fieldname;
        if (!doc[fieldname] && doc[fieldname] != 0) {
          has_value_in_second_column = false;
        }
      }
      for (let i2 = 0; i2 < this.columns.length; i2++) {
        let col = this.columns[i2];
        if (frappe.is_mobile() && col.type == "Field" && [3, 4].includes(i2)) {
          const no_seperator_class = !doc[(_a3 = col == null ? void 0 : col.df) == null ? void 0 : _a3.fieldname] ? "no-seperator" : "";
          left_html += `<div
					class="mobile-layout ${no_seperator_class} ${i2 == 3 ? "mobile-layout-seperator" : ""}"
					${no_seperator_class ? "style='padding-left: var(--margin-sm);'" : ""}
					>
					${this.get_column_html(col, doc, true)}
				</div>`;
        } else {
          left_html += this.get_column_html(col, doc, false);
        }
      }
      if (!has_value_in_second_column) {
        const container = document.createElement("div");
        container.innerHTML = left_html;
        const firstMobileLayout = container.querySelector(".mobile-layout");
        if (firstMobileLayout) {
          firstMobileLayout.classList.add("no-seperator");
        }
        left_html = container.innerHTML;
      }
      left_html += this.generate_button_html(doc);
      left_html += this.generate_dropdown_html(doc);
      return left_html;
    }
    get_right_html(doc) {
      return this.get_meta_html(doc);
    }
    get_list_row_html(doc) {
      return this.get_list_row_html_skeleton(this.get_left_html(doc), this.get_right_html(doc));
    }
    get_list_row_html_skeleton(left2 = "", right2 = "") {
      return `
			<div class="list-row-container" tabindex="1">
				<div class="level list-row">
					<div class="level-left ellipsis">
						${left2}
					</div>
					<div class="level-right text-muted ellipsis">
						${right2}
					</div>
				</div>
			</div>
		`;
    }
    get_column_html(col, doc, show_in_mobile) {
      var _a3, _b, _c, _d, _e;
      if (col.type === "Status" || ((_a3 = col.df) == null ? void 0 : _a3.options) == "Workflow State") {
        let show_workflow_state = ((_b = col.df) == null ? void 0 : _b.options) == "Workflow State";
        return `
				<div class="list-row-col hidden-xs ellipsis">
					${this.get_indicator_html(doc, show_workflow_state)}
				</div>
			`;
      }
      if (col.type === "Tag") {
        const tags_display_class = !this.tags_shown ? "hide" : "";
        let tags_html = doc._user_tags ? this.get_tags_html(doc._user_tags, 2, true) : '<div class="tags-empty">-</div>';
        return `
				<div class="list-row-col tag-col ${tags_display_class} hidden-xs ellipsis">
					${tags_html}
				</div>
			`;
      }
      const df = col.df || {};
      const label = df.label;
      const fieldname = df.fieldname;
      const link_title_fieldname = this.link_field_title_fields[fieldname];
      const value = doc[fieldname] || "";
      let value_display = link_title_fieldname ? doc[fieldname + "_" + link_title_fieldname] || value : value;
      let translated_doctypes = ((_c = frappe.boot) == null ? void 0 : _c.translated_doctypes) || [];
      if (translated_doctypes.includes(df.options)) {
        value_display = __(value_display);
      }
      const format = () => {
        if (df.fieldtype === "Percent") {
          return `<div class="progress" style="margin: 0px;">
						<div class="progress-bar progress-bar-success" role="progressbar"
							aria-valuenow="${value}"
							aria-valuemin="0" aria-valuemax="100" style="width: ${Math.round(value)}%;">
						</div>
					</div>`;
        } else {
          return frappe.format(value, df, null, doc);
        }
      };
      const field_html = () => {
        let html;
        let _value;
        let strip_html_required = df.fieldtype == "Text Editor" || df.fetch_from && ["Text", "Small Text"].includes(df.fieldtype);
        if (strip_html_required) {
          _value = strip_html(value_display);
        } else {
          _value = typeof value_display === "string" ? frappe.utils.escape_html(value_display) : value_display;
        }
        if (df.fieldtype === "Rating") {
          let out_of_ratings = df.options || 5;
          _value = _value * out_of_ratings;
        }
        let masked_fields = frappe.get_meta(this.doctype).masked_fields || [];
        let is_masked = masked_fields.includes(df.fieldname);
        let filterable = is_masked ? "no-underline" : " filterable";
        if (df.fieldtype === "Image") {
          html = df.options ? `<img src="${frappe.utils.escape_html(doc[df.options])}"
					style="max-height: 30px; max-width: 100%;">` : `<div class="missing-image small">
						${frappe.utils.icon("restriction")}
					</div>`;
        } else if (df.fieldtype === "Select") {
          html = `<span class="${filterable} indicator-pill ${frappe.utils.guess_colour(
            _value
          )} ellipsis"
					data-filter="${fieldname},=,${value}">
					<span class="ellipsis"> ${__(_value)} </span>
				</span>`;
        } else if (df.fieldtype === "Link") {
          html = `<a class="${filterable} ellipsis "
					data-filter="${fieldname},=,${value}">
					${_value}
				</a>`;
        } else if (frappe.model.html_fieldtypes.includes(df.fieldtype)) {
          html = `<span class="ellipsis">
					${_value}
				</span>`;
        } else if (df.fieldtype === "Percent") {
          return `<div style="width: 100%;"
					title="${__(label)}: ${frappe.utils.escape_html(_value)}">
					${format()}
				</div>`;
        } else {
          html = `<a class="${filterable} ellipsis"
					data-filter="${fieldname},=,${frappe.utils.escape_html(value)}">
					${format()}
				</a>`;
        }
        return `<span class="ellipsis"
				title="${__(label)}: ${frappe.utils.escape_html(_value)}">
				${html}
			</span>`;
      };
      const class_map = {
        Subject: "list-subject level",
        Field: !show_in_mobile ? "hidden-xs" : ""
      };
      let css_class = [
        "list-row-col ellipsis",
        class_map[col.type],
        frappe.model.is_numeric_field(df) ? "text-right" : "",
        fieldname
      ].join(" ");
      let column_html;
      if (this.settings.formatters && this.settings.formatters[fieldname] && col.type !== "Subject") {
        column_html = this.settings.formatters[fieldname](value, df, doc);
      } else {
        column_html = {
          Subject: this.get_subject_element(doc, value_display).innerHTML,
          Field: field_html()
        }[col.type];
      }
      if (frappe.is_mobile() && col.type == "Subject") {
        css_class += " bold";
      }
      let textLength = ((_e = (_d = $(column_html).text()) == null ? void 0 : _d.trim()) == null ? void 0 : _e.length) || 22.5;
      let calculatedWidth = textLength * 10 / 1.3 + (col.type == "Subject" ? 30 : 0);
      if ((!this.column_max_widths[fieldname] || calculatedWidth > this.column_max_widths[fieldname]) && !frappe.is_mobile()) {
        this.column_max_widths[fieldname] = calculatedWidth;
      }
      return `
			<div class="${css_class}">
				${column_html}
			</div>
		`;
    }
    apply_column_widths() {
      var _a3;
      if ((_a3 = this.list_view_settings) == null ? void 0 : _a3.disable_scrolling)
        return;
      Object.entries(this.column_max_widths).forEach(([fieldname, width]) => {
        $(`.list-view .frappe-list .result .level-left .list-row-col.${fieldname}`).css({
          width,
          flex: `1 0 ${width}px`
        });
      });
    }
    update_listview_classes(has_assignto, assign_to_count) {
      var _a3;
      if (has_assignto) {
        this.$result.addClass(["has-assign-to", `assign-to-length-${assign_to_count}`]);
        this.$result.removeClass("no-assign-to");
      } else {
        this.$result.removeClass("has-assign-to");
        this.$result.addClass("no-assign-to");
      }
      if (((_a3 = this.list_view_settings) == null ? void 0 : _a3.disable_scrolling) && !frappe.is_mobile()) {
        this.parent.page.main.parent().addClass("disable-scrolling");
      }
      let list_row = this.$result.find(".list-row-container .list-row").first();
      let frappe_list_width = this.$frappe_list.width();
      let left_width = list_row.find(".level-left").first().width();
      let right_width = list_row.find(".level-right").first().width();
      if (left_width < frappe_list_width - right_width) {
        this.$result.find(".list-row-container .list-row .level-right").addClass("border-0");
      }
    }
    get_tags_html(user_tags, limit = null, colored = false) {
      let get_tag_html = (tag) => {
        let color = "", style = "";
        if (tag) {
          if (colored) {
            color = frappe.get_palette(tag);
            style = `background-color: var(${color[0]}); color: var(${color[1]})`;
          }
          return `<div class="tag-pill ellipsis" title="${tag}" style="${style}">${tag}</div>`;
        }
      };
      user_tags = (user_tags || "").split(",");
      if (limit !== null) {
        user_tags = user_tags.slice(0, limit);
      }
      return user_tags.map(get_tag_html).join("");
    }
    get_meta_html(doc) {
      let html = "";
      const modified = comment_when(doc.modified, true);
      let assigned_to = ``;
      let assigned_users = doc._assign ? JSON.parse(doc._assign) : [];
      if (assigned_users.length) {
        assigned_to = `<div class="list-assignments d-flex align-items-center">
					${frappe.avatar_group(assigned_users, this.max_number_of_avatars - 1, {
          filterable: true
        })[0].outerHTML}
				</div>`;
      }
      let comment_count = null;
      if (this.list_view_settings && !this.list_view_settings.disable_comment_count) {
        comment_count = `<span class="comment-count d-flex align-items-center">
				${frappe.utils.icon("es-line-chat-alt")}
				${doc._comment_count > 99 ? "99+" : doc._comment_count || 0}
			</span>`;
      }
      html += `
			<div class="level-item list-row-activity hidden-xs">
				<div class="hidden-md hidden-xs d-flex">
					${assigned_to}
				</div>
				<span class="modified">${modified}</span>
				${comment_count || ""}
				${comment_count ? '<span class="mx-2">\xB7</span>' : ""}
				<span class="list-row-like hidden-xs" style="margin-bottom: 1px;">
					${this.get_like_html(doc)}
				</span>
			</div>
			<div class="level-item visible-xs text-right">
				${this.get_indicator_html(doc)}
			</div>
		`;
      return html;
    }
    generate_button_html(doc) {
      let button_container = "";
      if (this.settings.button) {
        const button_html = `
				<button class="btn btn-action btn-default btn-xs ellipsis"
					data-name="${doc.name}" data-idx="${doc._idx}"
					title="${this.settings.button.get_description(doc)}">
						${this.settings.button.get_label(doc)}
				</button>
			`;
        button_container += `
				<div class="list-row-col ellipsis hidden-xs">
					${this.settings.button.show(doc) ? button_html : "<span></span>"}
				</div>
			`;
      }
      return button_container;
    }
    generate_dropdown_html(doc) {
      let dropdown_container = "";
      if (this.settings.dropdown_button) {
        let button_actions = "";
        this.settings.dropdown_button.buttons.forEach((button, index) => {
          if (!button.show || button.show(doc)) {
            let description = button.get_description ? button.get_description(doc) : "";
            button_actions += `
						<a class="dropdown-item" href="#" onclick="return false;" data-idx="${doc._idx}" button-idx="${index}" title="${description}">
							${button.get_label}
						</a>
					`;
          }
        });
        let dropdown_buttons = "";
        if (button_actions) {
          dropdown_buttons = `
					<button type="button" class="btn btn-xs btn-default ellipsis" data-toggle="dropdown" aria-haspopup="true" aria-expanded="false">
						${this.settings.dropdown_button.get_label}
						${frappe.utils.icon("select", "xs")}
					</button>
					<div role="menu" class="dropdown-menu">${button_actions}</div>
				`;
        }
        dropdown_container = `
				<div class="list-row-col hidden-xs inner-group-button" data-name="${doc.name}" data-label="${this.settings.dropdown_button.get_label}">
					${dropdown_buttons}
				</div>
			`;
      }
      return dropdown_container;
    }
    apply_styles_basedon_dropdown() {
      if ($(".list-actions").length > 0 && $(".inner-group-button").length > 0) {
        $(".list-row .level-left, .list-row-head .level-left").css({
          flex: "2",
          "min-width": "72%"
        });
      }
    }
    get_count_str() {
      let current_count = this.data.length;
      let count_without_children = this.data.uniqBy((d) => d.name).length;
      return frappe.db.count(
        this.doctype,
        {
          filters: this.get_filters_for_args(),
          limit: this.count_upper_bound
        },
        Boolean(this.count_upper_bound)
      ).then((total_count) => {
        this.total_count = total_count;
        this.count_without_children = count_without_children !== current_count ? count_without_children : void 0;
        let count_str;
        if (current_count > this.total_count) {
          count_str = `${format_number(current_count, null, 0)}+`;
        } else if (this.total_count === this.count_upper_bound) {
          count_str = `${format_number(this.total_count - 1, null, 0)}+`;
        } else if (this.total_count == null) {
          count_str = "??";
        } else {
          count_str = format_number(this.total_count, null, 0);
        }
        let str = __("{0} of {1}", [format_number(current_count, null, 0), count_str]);
        if (this.count_without_children) {
          str = __("{0} of {1} ({2} rows with children)", [
            count_without_children,
            count_str,
            current_count
          ]);
        }
        return str;
      });
    }
    get_form_link(doc) {
      if (this.settings.get_form_link) {
        return this.settings.get_form_link(doc);
      }
      return `/desk/${encodeURIComponent(
        frappe.router.slug(frappe.router.doctype_layout || this.doctype)
      )}/${encodeURIComponent(cstr(doc.name))}`;
    }
    get_seen_class(doc) {
      const seen_by = doc._seen ? JSON.parse(doc._seen) : [];
      return seen_by.includes(frappe.session.user) ? "" : "bold";
    }
    get_like_html(doc) {
      const liked_by = doc._liked_by ? JSON.parse(doc._liked_by) : [];
      const is_liked = liked_by.includes(frappe.session.user);
      const title = liked_by.map((u) => frappe.user_info(u).fullname).join(", ");
      const div = document.createElement("div");
      div.appendChild(
        this._element_factory.get_like_element(doc.name, is_liked, liked_by, title)
      );
      return div.innerHTML;
    }
    get_subject_element(doc, title) {
      const ef = this._element_factory;
      const div = document.createElement("div");
      const checkboxspan = ef.get_checkboxspan_element();
      const ellipsisSpan = document.createElement("span");
      const seen = this.get_seen_class(doc);
      if (seen) {
        ellipsisSpan.classList.add("level-item", seen, "ellipsis");
      }
      div.appendChild(checkboxspan).appendChild(ef.get_checkbox_element(doc.name));
      div.appendChild(ellipsisSpan).appendChild(
        ef.get_link_element(
          doc.name,
          this.get_form_link(doc),
          this.get_subject_text(doc, title)
        )
      );
      return div;
    }
    get_subject_text(doc, title) {
      const subject_field = this.columns[0].df;
      let value = title || doc[subject_field.fieldname];
      if (this.settings.formatters && this.settings.formatters[subject_field.fieldname]) {
        let formatter = this.settings.formatters[subject_field.fieldname];
        value = formatter(value, subject_field, doc);
      }
      if (!value) {
        value = doc.name;
      }
      if (frappe.model.html_fieldtypes.includes(subject_field.fieldtype)) {
        return frappe.utils.html2text(value);
      } else {
        return value;
      }
    }
    get_indicator_html(doc, show_workflow_state) {
      const indicator = frappe.get_indicator(doc, this.doctype, show_workflow_state);
      const docstatus_description = [
        __("Document is in draft state"),
        __("Document has been submitted"),
        __("Document has been cancelled")
      ];
      const title = docstatus_description[doc.docstatus || 0];
      if (indicator) {
        return `<span class="indicator-pill ${indicator[1]} filterable no-indicator-dot ellipsis"
				data-filter='${indicator[2]}' title='${title}'>
				<span class="ellipsis"> ${indicator[0]}</span>
			</span>`;
      }
      return "";
    }
    get_indicator_dot(doc) {
      const indicator = frappe.get_indicator(doc, this.doctype);
      if (!indicator)
        return "";
      return `<span class='indicator ${indicator[1]}' title='${indicator[0]}'></span>`;
    }
    get_image_url(doc) {
      let url = doc.image ? doc.image : doc[this.meta.image_field];
      if (window.cordova && !frappe.utils.is_url(url)) {
        url = frappe.base_url + url;
      }
      return url || null;
    }
    setup_events() {
      this.setup_filterable();
      this.setup_sort_by();
      this.setup_list_click();
      this.setup_drag_click();
      this.setup_tag_visibility();
      this.setup_new_doc_event();
      this.setup_check_events();
      this.setup_like();
      this.setup_realtime_updates();
      this.setup_action_handler();
      this.setup_keyboard_navigation();
    }
    setup_keyboard_navigation() {
      let focus_first_row = () => {
        this.$result.find(".list-row-container:first").focus();
      };
      let focus_next = () => {
        $(document.activeElement).next().focus();
      };
      let focus_prev = () => {
        $(document.activeElement).prev().focus();
      };
      let list_row_focused = () => {
        return $(document.activeElement).is(".list-row-container");
      };
      let check_row = ($row) => {
        let $input = $row.find("input[type=checkbox]");
        $input.click();
      };
      let get_list_row_if_focused = () => list_row_focused() ? $(document.activeElement) : null;
      let is_current_page = () => this.page.wrapper.is(":visible");
      let is_input_focused = () => $(document.activeElement).is("input");
      let handle_navigation = (direction) => {
        if (!is_current_page() || is_input_focused())
          return false;
        let $list_row = get_list_row_if_focused();
        if ($list_row) {
          direction === "down" ? focus_next() : focus_prev();
        } else {
          focus_first_row();
        }
      };
      frappe.ui.keys.add_shortcut({
        shortcut: "down",
        action: () => handle_navigation("down"),
        description: __("Navigate list down", null, "Description of a list view shortcut"),
        page: this.page
      });
      frappe.ui.keys.add_shortcut({
        shortcut: "up",
        action: () => handle_navigation("up"),
        description: __("Navigate list up", null, "Description of a list view shortcut"),
        page: this.page
      });
      frappe.ui.keys.add_shortcut({
        shortcut: "shift+down",
        action: () => {
          if (!is_current_page() || is_input_focused())
            return false;
          let $list_row = get_list_row_if_focused();
          check_row($list_row);
          focus_next();
        },
        description: __(
          "Select multiple list items",
          null,
          "Description of a list view shortcut"
        ),
        page: this.page
      });
      frappe.ui.keys.add_shortcut({
        shortcut: "shift+up",
        action: () => {
          if (!is_current_page() || is_input_focused())
            return false;
          let $list_row = get_list_row_if_focused();
          check_row($list_row);
          focus_prev();
        },
        description: __(
          "Select multiple list items",
          null,
          "Description of a list view shortcut"
        ),
        page: this.page
      });
      frappe.ui.keys.add_shortcut({
        shortcut: "enter",
        action: () => {
          let $list_row = get_list_row_if_focused();
          if ($list_row) {
            $list_row.find("a[data-name]")[0].click();
            return true;
          }
          return false;
        },
        description: __("Open list item", null, "Description of a list view shortcut"),
        page: this.page
      });
      frappe.ui.keys.add_shortcut({
        shortcut: "space",
        action: () => {
          let $list_row = get_list_row_if_focused();
          if ($list_row) {
            check_row($list_row);
            return true;
          }
          return false;
        },
        description: __("Select list item", null, "Description of a list view shortcut"),
        page: this.page
      });
    }
    setup_filterable() {
      this.$result.on("click", ".filterable", (e) => {
        if (e.metaKey || e.ctrlKey)
          return;
        e.stopPropagation();
        const $this = $(e.currentTarget);
        const filters = $this.attr("data-filter").split("|");
        const filters_to_apply = filters.map((f) => {
          f = f.split(",");
          if (f[2] === "Today") {
            f[2] = frappe.datetime.get_today();
          } else if (f[2] == "User") {
            f[2] = frappe.session.user;
          }
          this.filter_area.remove(f[0]);
          return [this.doctype, f[0], f[1], f.slice(2).join(",")];
        });
        this.filter_area.add(filters_to_apply);
      });
    }
    setup_sort_by() {
      this.$result.on("click", "[data-sort-by]", (e) => {
        const sort_by = e.currentTarget.getAttribute("data-sort-by");
        if (!sort_by)
          return;
        let sort_order = "asc";
        if (this.sort_by === sort_by) {
          sort_order = this.sort_order === "asc" ? "desc" : "asc";
        }
        this.sort_selector.set_value(sort_by, sort_order);
        this.on_sort_change(sort_by, sort_order);
      });
    }
    setup_list_click() {
      this.$result.on("click", ".list-row, .image-view-header, .file-header", (e) => {
        const $target = $(e.target);
        if ((e.ctrlKey || e.metaKey) && !$target.is("a")) {
          const $list_row = $(e.currentTarget);
          const $check = $list_row.find(".list-row-checkbox");
          $check.prop("checked", !$check.prop("checked"));
          e.preventDefault();
          this.on_row_checked();
          return;
        }
        if ($target.is("[data-toggle='dropdown']"))
          return true;
        if ($target.hasClass("filterable") || $target.hasClass("select-like") || $target.hasClass("file-select") || $target.hasClass("list-row-like") || $target.is(":checkbox")) {
          e.stopPropagation();
          return;
        }
        if ($target.is("a"))
          return;
        const $row = $(e.currentTarget);
        const link = $row.find(".list-subject a").get(0);
        if (link) {
          frappe.set_route(link.pathname);
          return false;
        }
      });
    }
    setup_drag_click() {
      this.dragClick = false;
      this.$result.on("mousedown", ".list-row-checkbox", (e) => {
        var _a3, _b;
        (_a3 = e.stopPropagation) == null ? void 0 : _a3.call(e);
        (_b = e.preventDefault) == null ? void 0 : _b.call(e);
        this.dragClick = true;
        this.check = !e.target.checked;
      });
      $(document).on("mouseup", () => {
        this.dragClick = false;
      });
      this.$result.on("mousemove", ".level.list-row", (e) => {
        if (this.dragClick) {
          this.check_row_on_drag(e, this.check);
        }
      });
    }
    check_row_on_drag(event2, check = true) {
      $(event2.target).find(".list-row-checkbox").prop("checked", check);
      this.on_row_checked();
    }
    setup_action_handler() {
      this.$result.on("click", ".btn-action", (e) => {
        const $button = $(e.currentTarget);
        const doc = this.data[$button.attr("data-idx")];
        this.settings.button.action(doc);
        e.stopPropagation();
        return false;
      });
      this.$result.on("click", ".inner-group-button .dropdown-item", (e) => {
        const $button = $(e.currentTarget);
        const doc = this.data[$button.attr("data-idx")];
        const btn_idx = parseInt($button.attr("button-idx"), 10);
        const button = this.settings.dropdown_button.buttons[btn_idx];
        if (button && button.action) {
          button.action(doc);
        }
        e.stopPropagation();
        return false;
      });
    }
    setup_check_events() {
      this.$result.on("change", "input[type=checkbox]", (e) => {
        const $target = $(e.currentTarget);
        if ($target.is(".list-header-subject .list-check-all")) {
          const $check = this.$result.find(".checkbox-actions .list-check-all");
          $check.prop("checked", $target.prop("checked"));
          $check.trigger("change");
        } else if ($target.is(".checkbox-actions .list-check-all")) {
          const $check = this.$result.find(".list-header-subject .list-check-all");
          $check.prop("checked", $target.prop("checked"));
          this.$result.find(".list-row-checkbox").prop("checked", $target.prop("checked"));
        } else if ($target.attr("data-parent")) {
          this.$result.find(`.${$target.attr("data-parent")}`).find(".list-row-checkbox").prop("checked", $target.prop("checked"));
        }
        this.on_row_checked();
      });
      this.$result.on("click", ".list-row-checkbox", (e) => {
        const $target = $(e.currentTarget);
        if (e.shiftKey && this.$checkbox_cursor && !$target.is(this.$checkbox_cursor)) {
          const name_1 = decodeURIComponent(this.$checkbox_cursor.data().name);
          const name_2 = decodeURIComponent($target.data().name);
          const index_1 = this.data.findIndex((d) => d.name === name_1);
          const index_2 = this.data.findIndex((d) => d.name === name_2);
          let [min_index, max_index] = [index_1, index_2];
          if (min_index > max_index) {
            [min_index, max_index] = [max_index, min_index];
          }
          let docnames = this.data.slice(min_index + 1, max_index).map((d) => d.name);
          const selector = docnames.map((name) => `.list-row-checkbox[data-name="${encodeURIComponent(name)}"]`).join(",");
          this.$result.find(selector).prop("checked", true);
        }
        this.$checkbox_cursor = $target;
        this.update_checkbox($target);
      });
      let me2 = this;
      this.page.actions_btn_group.on("show.bs.dropdown", () => {
        me2.toggle_workflow_actions();
      });
    }
    setup_like() {
      this.$result.on("click", ".like-action", (e) => {
        const $this = $(e.currentTarget);
        const { doctype, name } = $this.data();
        frappe.ui.toggle_like($this, doctype, name);
        return false;
      });
      this.$result.on("click", ".list-liked-by-me", (e) => {
        const $this = $(e.currentTarget);
        $this.toggleClass("liked");
        if ($this.hasClass("liked")) {
          this.filter_area.add(
            this.doctype,
            "_liked_by",
            "like",
            "%" + frappe.session.user + "%"
          );
        } else {
          this.filter_area.remove("_liked_by");
        }
      });
    }
    setup_new_doc_event() {
      this.$no_result.find(".btn-new-doc").click(() => {
        if (this.settings.primary_action) {
          this.settings.primary_action();
        } else {
          this.make_new_doc();
        }
      });
    }
    setup_tag_visibility() {
      var _a3;
      this.tags_shown = (_a3 = this.list_view_settings) == null ? void 0 : _a3.show_tags;
    }
    setup_realtime_updates() {
      var _a3;
      this.pending_document_refreshes = [];
      if (((_a3 = this.list_view_settings) == null ? void 0 : _a3.disable_auto_refresh) || this.realtime_events_setup) {
        return;
      }
      frappe.realtime.doctype_subscribe(this.doctype);
      frappe.realtime.off("list_update");
      frappe.realtime.on("list_update", (data) => {
        if ((data == null ? void 0 : data.doctype) !== this.doctype) {
          return;
        }
        if (this.$checks && this.$checks.length) {
          return;
        }
        if (this.avoid_realtime_update()) {
          return;
        }
        this.pending_document_refreshes.push(data);
        this.debounced_refresh();
      });
      this.realtime_events_setup = true;
    }
    disable_realtime_updates() {
      frappe.realtime.doctype_unsubscribe(this.doctype);
      this.realtime_events_setup = false;
    }
    process_document_refreshes() {
      if (!this.pending_document_refreshes.length)
        return;
      const route = frappe.get_route() || [];
      if (!cur_list || route[0] != "List" || cur_list.doctype != route[1]) {
        this.pending_document_refreshes = [];
        this.disable_realtime_updates();
        return;
      }
      const names = this.pending_document_refreshes.map((d) => d.name);
      this.pending_document_refreshes = this.pending_document_refreshes.filter(
        (d) => names.indexOf(d.name) === -1
      );
      if (!names.length)
        return;
      const call_args = this.get_call_args();
      call_args.args.filters.push([this.doctype, "name", "in", names]);
      call_args.args.start = 0;
      frappe.call(call_args).then(({ message }) => {
        if (!message)
          return;
        const data = frappe.utils.dict(message.keys, message.values);
        if (!(data && data.length)) {
          this.data = this.data.filter((d) => !names.includes(d.name));
          this.remove_list_items(names);
          return;
        }
        data.forEach((datum) => {
          const index = this.data.findIndex((doc) => doc.name === datum.name);
          if (index === -1) {
            this.data.push(datum);
          } else {
            this.data[index] = datum;
          }
        });
        this.data.sort((a, b) => {
          const a_value = a[this.sort_by] || "";
          const b_value = b[this.sort_by] || "";
          let return_value = 0;
          if (a_value > b_value) {
            return_value = 1;
          }
          if (b_value > a_value) {
            return_value = -1;
          }
          if (this.sort_order === "desc") {
            return_value = -return_value;
          }
          return return_value;
        });
        if (this.$checks && this.$checks.length) {
          this.set_rows_as_checked();
        }
        this.toggle_result_area();
        this.render_list();
      });
    }
    avoid_realtime_update() {
      var _a3;
      if ((_a3 = this.filter_area) == null ? void 0 : _a3.is_being_edited()) {
        return true;
      }
      if (this.disable_list_update) {
        return true;
      }
      return false;
    }
    remove_list_items(names) {
      for (let name of names) {
        this.$result.find(`.list-row-checkbox[data-name='${name.replace(/'/g, "\\'")}']`).closest(".list-row-container").remove();
      }
    }
    set_rows_as_checked() {
      if (!this.$checks || !this.$checks.length) {
        return;
      }
      $.each(this.$checks, (i2, el) => {
        let docname = $(el).attr("data-name");
        this.$result.find(`.list-row-checkbox[data-name='${docname}']`).prop("checked", true);
      });
      this.on_row_checked();
    }
    on_row_checked() {
      this.$list_head_subject = this.$list_head_subject || this.$result.find("header .list-header-subject");
      this.$checkbox_actions = this.$checkbox_actions || this.$result.find("header .checkbox-actions");
      this.$checks = this.$result.find(".list-row-checkbox:checked");
      this.$list_head_subject.toggle(this.$checks.length === 0);
      this.$checkbox_actions.toggle(this.$checks.length > 0);
      if (this.$checks.length === 0) {
        this.$list_head_subject.find(".list-check-all").prop("checked", false);
      } else {
        this.$checkbox_actions.find(".list-header-meta").html(__("{0} items selected", [this.$checks.length]));
        this.$checkbox_actions.show();
        this.$list_head_subject.hide();
      }
      this.update_checkbox();
      this.toggle_actions_menu_button(this.$checks.length > 0);
    }
    get_checked_items(only_docnames) {
      const docnames = Array.from(this.$checks || []).map(
        (check) => cstr(unescape($(check).data().name))
      );
      if (only_docnames)
        return docnames;
      return this.data.filter((d) => docnames.includes(d.name));
    }
    clear_checked_items() {
      this.$checks && this.$checks.prop("checked", false);
      this.on_row_checked();
    }
    save_view_user_settings(obj) {
      return frappe.model.user_settings.save(this.doctype, this.view_name, obj);
    }
    on_update() {
    }
    update_url_with_filters() {
      if (frappe.get_route_str() == this.page_name && !this.report_name) {
        window.history.replaceState(null, null, this.get_url_with_filters());
      }
    }
    get_url_with_filters() {
      let search_params = this.get_search_params();
      let full_url = window.location.href.replace(window.location.search, "");
      if (search_params.size) {
        full_url += "?" + search_params.toString();
      }
      return full_url;
    }
    get_search_params() {
      let search_params = new URLSearchParams();
      this.get_filters_for_args().forEach((filter) => {
        const doctype = filter[0];
        const field = filter[1];
        const operator = filter[2];
        const value = filter[3];
        const query_key = doctype === this.doctype ? field : `${doctype}.${field}`;
        const query_value = operator === "=" ? value : JSON.stringify([operator, value]);
        search_params.append(query_key, query_value);
      });
      return search_params;
    }
    get_menu_items() {
      const doctype = this.doctype;
      const items = [];
      if (frappe.model.can_import(doctype, null, this.meta)) {
        items.push({
          label: __("Import", null, "Button in list view menu"),
          action: () => frappe.set_route("list", "data-import", {
            reference_doctype: doctype
          }),
          standard: true
        });
      }
      if (frappe.user_roles.includes("System Manager")) {
        items.push({
          label: __("User Permissions", null, "Button in list view menu"),
          action: () => frappe.set_route("list", "user-permission", {
            allow: doctype
          }),
          standard: true
        });
      }
      if (frappe.user_roles.includes("System Manager")) {
        items.push({
          label: __("Role Permissions Manager", null, "Button in list view menu"),
          action: () => frappe.set_route("permission-manager", {
            doctype
          }),
          standard: true
        });
      }
      if (frappe.model.can_create("Custom Field") && frappe.model.can_create("Property Setter") && !frappe.model.core_doctypes_list.includes(doctype)) {
        items.push({
          label: __("Customize", null, "Button in list view menu"),
          action: () => {
            if (!this.meta)
              return;
            if (this.meta.custom) {
              frappe.set_route("form", "doctype", doctype);
            } else if (!this.meta.custom) {
              frappe.set_route("form", "customize-form", {
                doc_type: doctype
              });
            }
          },
          standard: true,
          shortcut: "Ctrl+Y"
        });
      }
      if (frappe.user.has_role("System Manager") && frappe.boot.developer_mode) {
        items.push({
          label: __("Edit DocType", null, "Button in list view menu"),
          action: () => frappe.set_route("form", "doctype", doctype),
          standard: true
        });
      }
      items.push({
        label: __("Customize Quick Filters", null, "Customize qucik filters of List View"),
        action: () => {
          this.make_group_by_fields_modal();
        },
        standard: true
      });
      if (frappe.user.has_role("System Manager")) {
        if (this.get_view_settings) {
          items.push(this.get_view_settings());
        }
      }
      return items;
    }
    make_group_by_fields_modal() {
      let d = new frappe.ui.Dialog({
        title: __("Select Filters"),
        fields: this.get_group_by_dropdown_fields()
      });
      d.set_primary_action(__("Save"), ({ group_by_fields }) => {
        frappe.model.user_settings.save(
          this.doctype,
          "group_by_fields",
          group_by_fields || null
        );
        this.group_by_fields = group_by_fields ? ["assigned_to", "owner", ...group_by_fields] : ["assigned_to", "owner"];
        d.hide();
        frappe.msgprint(__("Saving Changes..."));
        setTimeout(() => {
          location.reload();
        }, 1500);
      });
      d.$body.prepend(`
			<div class="filters-search">
				<input type="text"
					placeholder="${__("Search")}"
					data-element="search" class="form-control input-xs">
			</div>
		`);
      frappe.utils.setup_search(d.$body, ".unit-checkbox", ".label-area");
      d.show();
    }
    get_group_by_dropdown_fields() {
      var _a3;
      let group_by_fields = [];
      let default_fields = ((_a3 = frappe.get_user_settings(this.doctype)) == null ? void 0 : _a3.group_by_fields) || [];
      let fields = this.meta.fields.filter(
        (f) => ["Select", "Link", "Data", "Int", "Check"].includes(f.fieldtype)
      );
      let default_fields_dict = [
        {
          label: "Assigned To",
          fieldname: "assigned_to"
        },
        {
          label: "Created By",
          fieldname: "owner"
        },
        {
          label: "Tags",
          fieldname: "tags"
        }
      ];
      fields = fields.concat(default_fields_dict);
      group_by_fields.push({
        label: __(this.doctype),
        fieldname: "group_by_fields",
        fieldtype: "MultiCheck",
        columns: 2,
        options: fields.map((df) => ({
          label: __(df.label, null, df.parent),
          value: df.fieldname,
          checked: default_fields.includes(df.fieldname)
        }))
      });
      return group_by_fields;
    }
    get_view_settings() {
      return {
        label: __("List Settings", null, "Button in list view menu"),
        action: () => this.show_list_settings(),
        standard: true
      };
    }
    show_list_settings() {
      frappe.model.with_doctype(this.doctype, () => {
        new ListSettings({
          listview: this,
          doctype: this.doctype,
          settings: this.list_view_settings,
          meta: frappe.get_meta(this.doctype)
        });
      });
    }
    get_workflow_action_menu_items() {
      const workflow_actions = [];
      const me2 = this;
      if (frappe.model.has_workflow(this.doctype)) {
        const actions = frappe.workflow.get_all_transition_actions(this.doctype);
        actions.forEach((action) => {
          workflow_actions.push({
            label: __(action),
            name: action,
            action: () => {
              me2.disable_list_update = true;
              frappe.xcall("frappe.model.workflow.bulk_workflow_approval", {
                docnames: this.get_checked_items(true),
                doctype: this.doctype,
                action
              }).finally(() => {
                me2.disable_list_update = false;
              });
            },
            is_workflow_action: true
          });
        });
      }
      return workflow_actions;
    }
    toggle_workflow_actions() {
      if (!frappe.model.has_workflow(this.doctype))
        return;
      Object.keys(this.workflow_action_items).forEach((key) => {
        this.workflow_action_items[key].addClass("disabled");
      });
      const checked_items = this.get_checked_items();
      frappe.xcall("frappe.model.workflow.get_common_transition_actions", {
        docs: checked_items,
        doctype: this.doctype
      }).then((actions) => {
        Object.keys(this.workflow_action_items).forEach((key) => {
          const $link = this.workflow_action_items[key];
          const $item = $link == null ? void 0 : $link.closest("li");
          $link == null ? void 0 : $link.removeClass("disabled");
          $link == null ? void 0 : $link.toggle(actions.includes(key));
          $item == null ? void 0 : $item.toggle(actions.includes(key));
        });
      });
    }
    get_actions_menu_items() {
      const doctype = this.doctype;
      const actions_menu_items = [];
      const bulk_operations = new BulkOperations({ doctype: this.doctype });
      const is_field_editable = (field_doc) => {
        return field_doc.fieldname && frappe.model.is_value_type(field_doc) && field_doc.fieldtype !== "Read Only" && !field_doc.hidden && !field_doc.read_only && !field_doc.is_virtual;
      };
      const has_editable_fields = (doctype2) => {
        return frappe.meta.get_docfields(doctype2).some((field_doc) => is_field_editable(field_doc));
      };
      const has_submit_permission = (doctype2) => {
        return frappe.perm.has_perm(doctype2, 0, "submit");
      };
      const is_bulk_edit_allowed = (doctype2) => {
        var _a3;
        if (frappe.model.has_workflow(doctype2)) {
          return !!((_a3 = this.list_view_settings) == null ? void 0 : _a3.allow_edit);
        }
        return true;
      };
      const bulk_assignment = () => {
        return {
          label: __("Assign To", null, "Button in list view actions menu"),
          action: () => {
            this.disable_list_update = true;
            bulk_operations.assign(this.get_checked_items(true), () => {
              this.disable_list_update = false;
              this.clear_checked_items();
              this.refresh();
            });
          },
          standard: true
        };
      };
      const bulk_assignment_clear = () => {
        return {
          label: __("Clear Assignment", null, "Button in list view actions menu"),
          action: () => {
            frappe.confirm(__("Are you sure you want to clear the assignments?"), () => {
              this.disable_list_update = true;
              bulk_operations.clear_assignment(this.get_checked_items(true), () => {
                this.disable_list_update = false;
                this.clear_checked_items();
                this.refresh();
              });
            });
          },
          standard: true
        };
      };
      const bulk_assignment_rule = () => {
        return {
          label: __("Apply Assignment Rule", null, "Button in list view actions menu"),
          action: () => {
            this.disable_list_update = true;
            bulk_operations.apply_assignment_rule(this.get_checked_items(true), () => {
              this.disable_list_update = false;
              this.clear_checked_items();
              this.refresh();
            });
          },
          standard: true
        };
      };
      const bulk_add_tags = () => {
        return {
          label: __("Add Tags", null, "Button in list view actions menu"),
          action: () => {
            this.disable_list_update = true;
            bulk_operations.add_tags(this.get_checked_items(true), () => {
              this.disable_list_update = false;
              this.clear_checked_items();
              this.refresh();
            });
          },
          standard: true
        };
      };
      const bulk_printing = () => {
        return {
          label: __("Print", null, "Button in list view actions menu"),
          action: () => bulk_operations.print(this.get_checked_items()),
          standard: true
        };
      };
      const bulk_delete = () => {
        return {
          label: __("Delete", null, "Button in list view actions menu"),
          action: () => {
            const docnames = this.get_checked_items(true).map(
              (docname) => docname.toString()
            );
            let message = __(
              "Delete {0} item permanently?",
              [docnames.length],
              "Title of confirmation dialog"
            );
            if (docnames.length > 1) {
              message = __(
                "Delete {0} items permanently?",
                [docnames.length],
                "Title of confirmation dialog"
              );
            }
            frappe.confirm(message, () => {
              this.disable_list_update = true;
              bulk_operations.delete(docnames, () => {
                this.disable_list_update = false;
                this.clear_checked_items();
                this.refresh();
              });
            });
          },
          standard: true
        };
      };
      const bulk_cancel = () => {
        return {
          label: __("Cancel", null, "Button in list view actions menu"),
          action: () => {
            const docnames = this.get_checked_items(true);
            if (docnames.length > 0) {
              frappe.confirm(
                __(
                  "Cancel {0} documents?",
                  [docnames.length],
                  "Title of confirmation dialog"
                ),
                () => {
                  this.disable_list_update = true;
                  bulk_operations.submit_or_cancel(docnames, "cancel", () => {
                    this.disable_list_update = false;
                    this.clear_checked_items();
                    this.refresh();
                  });
                }
              );
            }
          },
          standard: true
        };
      };
      const bulk_submit = () => {
        return {
          label: __("Submit", null, "Button in list view actions menu"),
          action: () => {
            const docnames = this.get_checked_items(true);
            if (docnames.length > 0) {
              frappe.confirm(
                __(
                  "Submit {0} documents?",
                  [docnames.length],
                  "Title of confirmation dialog"
                ),
                () => {
                  this.disable_list_update = true;
                  bulk_operations.submit_or_cancel(docnames, "submit", () => {
                    this.disable_list_update = false;
                    this.clear_checked_items();
                    this.refresh();
                  });
                }
              );
            }
          },
          standard: true
        };
      };
      const bulk_edit = () => {
        return {
          label: __("Edit", null, "Button in list view actions menu"),
          action: () => {
            let field_mappings = {};
            frappe.meta.get_docfields(doctype).forEach((field_doc) => {
              if (is_field_editable(field_doc)) {
                const field_key = `${field_doc.label} (${doctype})`;
                field_mappings[field_key] = Object.assign({}, field_doc, {
                  is_child_field: false
                });
              }
              if (field_doc.fieldtype === "Table" && field_doc.options) {
                const child_doctype = field_doc.options;
                const child_fields = frappe.meta.get_docfields(child_doctype);
                child_fields.forEach((child_field) => {
                  if (is_field_editable(child_field)) {
                    const field_key = `${child_field.label} (${field_doc.label})`;
                    field_mappings[field_key] = Object.assign({}, child_field, {
                      is_child_field: true,
                      child_doctype,
                      parent_table_field: field_doc.fieldname
                    });
                  }
                });
              }
            });
            this.disable_list_update = true;
            bulk_operations.edit(this.get_checked_items(true), field_mappings, () => {
              this.disable_list_update = false;
              this.refresh();
            });
          },
          standard: true
        };
      };
      const bulk_export = () => {
        return {
          label: __("Export", null, "Button in list view actions menu"),
          action: () => {
            const docnames = this.get_checked_items(true);
            bulk_operations.export(doctype, docnames);
          },
          standard: true
        };
      };
      const copy_to_clipboard = () => {
        return {
          label: __("Copy to Clipboard"),
          action: () => {
            const selected_items = this.get_checked_items();
            if (selected_items.length === 0) {
              frappe.show_alert({
                message: __("No rows selected"),
                indicator: "orange"
              });
              return;
            }
            let columns;
            if (this.columns && this.columns.length && (this.columns[0].docfield || this.columns[0].type == "Status")) {
              columns = this.columns.map((col) => ({
                fieldname: col.id || col.field,
                label: col.content || col.name,
                docfield: col.docfield
              }));
            } else if (this.columns && this.columns.length && this.columns[0].type) {
              columns = this.columns.filter((col) => {
                return col.df && col.df.fieldname || col.type === "Subject" || col.type === "Status";
              }).map((col) => {
                var _a3, _b;
                if (col.type === "Subject") {
                  return {
                    fieldname: ((_a3 = col.df) == null ? void 0 : _a3.fieldname) || "name",
                    label: __(((_b = col.df) == null ? void 0 : _b.label) || "ID"),
                    type: "Subject"
                  };
                } else if (col.type === "Status") {
                  return {
                    fieldname: "status",
                    label: __("Status"),
                    type: "Status"
                  };
                } else {
                  return {
                    fieldname: col.df.fieldname,
                    label: __(col.df.label || col.df.fieldname),
                    type: col.type
                  };
                }
              });
            }
            const headers = columns.map((col) => col.label).join("	");
            const rows = selected_items.map((item) => {
              return columns.map((col) => {
                var _a3, _b;
                let value;
                const df = col.df || col.docfield;
                if (col.type === "Status" || col.fieldname === "docstatus") {
                  const indicator = frappe.get_indicator(item, this.doctype);
                  if (indicator && indicator.length > 0) {
                    value = indicator[0];
                  } else {
                    value = item.status || "";
                  }
                } else {
                  const link_title_fieldname = (_a3 = this.link_field_title_fields) == null ? void 0 : _a3[col.fieldname];
                  if (link_title_fieldname) {
                    value = item[col.fieldname + "_" + link_title_fieldname] || item[col.fieldname];
                  } else if (df && df.fieldtype === "Link" && df.options && item[col.fieldname]) {
                    if ((_b = frappe.boot.link_title_doctypes) == null ? void 0 : _b.includes(df.options)) {
                      let link_title = frappe.utils.get_link_title(
                        df.options,
                        item[col.fieldname]
                      );
                      value = link_title || item[col.fieldname];
                    } else {
                      value = item[col.fieldname];
                    }
                  } else {
                    value = item[col.fieldname];
                  }
                }
                if (value == null)
                  return "";
                return String(value).replace(/<[^>]*>/g, "");
              }).join("	");
            });
            const clipboard_data = [headers, ...rows].join("\n");
            const message = __("Copied {0} {1} to clipboard", [
              selected_items.length,
              selected_items.length === 1 ? __("row") : __("rows")
            ]);
            frappe.utils.copy_to_clipboard(clipboard_data, message);
          },
          standard: true
        };
      };
      actions_menu_items.push(copy_to_clipboard());
      if (has_editable_fields(doctype) && is_bulk_edit_allowed(doctype)) {
        actions_menu_items.push(bulk_edit());
      }
      actions_menu_items.push(bulk_export());
      actions_menu_items.push(bulk_assignment());
      actions_menu_items.push(bulk_assignment_clear());
      actions_menu_items.push(bulk_assignment_rule());
      actions_menu_items.push(bulk_add_tags());
      if (frappe.model.can_print(doctype)) {
        actions_menu_items.push(bulk_printing());
      }
      if (frappe.model.is_submittable(doctype) && has_submit_permission(doctype) && !frappe.model.has_workflow(doctype)) {
        actions_menu_items.push(bulk_submit());
      }
      if (frappe.model.can_cancel(doctype) && !frappe.model.has_workflow(doctype)) {
        actions_menu_items.push(bulk_cancel());
      }
      if (frappe.model.can_delete(doctype) && is_bulk_edit_allowed(doctype)) {
        actions_menu_items.push(bulk_delete());
      }
      return actions_menu_items;
    }
    parse_filters_from_route_options() {
      const filters = [];
      let params = new URLSearchParams(window.location.search);
      if (!params.toString() && frappe.route_options) {
        params = new Map(Object.entries(frappe.route_options));
      }
      params.forEach((value, field) => {
        let doctype = null;
        let value_array;
        if (Array.isArray(value) && !Array.isArray(value[0]) && value[0].startsWith("[") && value[0].endsWith("]")) {
          value_array = [];
          for (var i2 = 0; i2 < value.length; i2++) {
            value_array.push(JSON.parse(value[i2]));
          }
        } else if (typeof value === "string" && value.startsWith("[") && value.endsWith("]")) {
          value = JSON.parse(value);
        }
        if (field.includes(".")) {
          doctype = field.split(".")[0];
          field = field.split(".")[1];
        }
        if (!doctype) {
          doctype = frappe.meta.get_doctype_for_field(this.doctype, field);
        }
        if (doctype) {
          if (value_array) {
            for (var j = 0; j < value_array.length; j++) {
              if (Array.isArray(value_array[j])) {
                filters.push([doctype, field, value_array[j][0], value_array[j][1]]);
              } else {
                filters.push([doctype, field, "=", value_array[j]]);
              }
            }
          } else if (Array.isArray(value) && Array.isArray(value[0])) {
            value.forEach((val) => {
              filters.push([doctype, field, val[0], val[1]]);
            });
          } else if (Array.isArray(value)) {
            filters.push([doctype, field, value[0], value[1]]);
          } else {
            filters.push([doctype, field, "=", value]);
          }
        }
      });
      return filters;
    }
  };
  frappe.get_list_view = (doctype) => {
    let route = `List/${doctype}/List`;
    return frappe.views.list_view[route];
  };
  var ElementFactory = class {
    constructor(doctype) {
      this.templates = {
        checkbox: this.create_checkbox_element(doctype),
        checkboxspan: this.create_checkboxspan_element(),
        link: this.create_link_element(doctype),
        like: this.create_like_element(doctype)
      };
    }
    create_checkbox_element(doctype) {
      const checkbox = document.createElement("input");
      checkbox.classList.add("list-row-checkbox");
      checkbox.type = "checkbox";
      checkbox.dataset.doctype = doctype;
      return checkbox;
    }
    create_link_element(doctype) {
      const link = document.createElement("a");
      link.classList.add("ellipsis");
      link.dataset.doctype = doctype;
      return link;
    }
    create_checkboxspan_element() {
      const checkboxspan = document.createElement("span");
      checkboxspan.classList.add("level-item", "select-like");
      return checkboxspan;
    }
    create_like_element(doctype) {
      const like = document.createElement("span");
      like.classList.add("like-action");
      like.innerHTML = `<svg class="icon icon-sm like-icon"><use href="#icon-heart"></use></svg>`;
      like.dataset.doctype = doctype;
      return like;
    }
    get_checkbox_element(name) {
      const checkbox = this.templates.checkbox.cloneNode(true);
      checkbox.dataset.name = name;
      return checkbox;
    }
    get_checkboxspan_element() {
      return this.templates.checkboxspan.cloneNode(true);
    }
    get_link_element(name, href, text) {
      const link = this.templates.link.cloneNode(true);
      link.dataset.name = name;
      link.href = href;
      link.title = text;
      link.textContent = text;
      return link;
    }
    get_like_element(name, liked, liked_by, title) {
      const like = this.templates.like.cloneNode(true);
      like.dataset.name = name;
      const heart_classes = liked ? ["liked-by", "liked"] : ["not-liked"];
      like.classList.add(...heart_classes);
      like.setAttribute("data-liked-by", liked_by || "[]");
      like.setAttribute("title", title);
      return like;
    }
  };

  // frappe/public/js/frappe/list/list_factory.js
  frappe.provide("frappe.views.list_view");
  window.cur_list = null;
  frappe.views.ListFactory = class ListFactory extends frappe.views.Factory {
    make(route) {
      const me2 = this;
      const doctype = route[1];
      let view_name = frappe.utils.to_title_case(route[2] || "List");
      if (doctype == "File" && !["Report", "Dashboard"].includes(view_name)) {
        view_name = "File";
      }
      let view_class = frappe.views[view_name + "View"];
      if (!view_class)
        view_class = frappe.views.ListView;
      if (view_class && view_class.load_last_view && view_class.load_last_view()) {
        return;
      }
      frappe.provide("frappe.views.list_view." + doctype);
      const hide_sidebar = true;
      frappe.views.list_view[me2.page_name] = new view_class({
        doctype,
        parent: me2.make_page(true, me2.page_name, hide_sidebar ? null : "Right")
      });
      me2.set_cur_list();
    }
    before_show() {
      if (this.re_route_to_view()) {
        return false;
      }
      this.set_module_breadcrumb();
    }
    on_show() {
      this.set_cur_list();
      if (cur_list)
        cur_list.show();
    }
    re_route_to_view() {
      const doctype = this.route[1];
      const last_route = frappe.route_history.slice(-2)[0];
      if (this.route[0] === "List" && this.route.length === 2 && frappe.views.list_view[doctype] && last_route && last_route[0] === "List" && last_route[1] === doctype) {
        window.history.go(-1);
        return true;
      }
    }
    set_module_breadcrumb() {
      if (frappe.route_history.length > 1) {
        const prev_route = frappe.route_history[frappe.route_history.length - 2];
        if (prev_route[0] === "modules") {
          const doctype = this.route[1], module2 = prev_route[1];
          if (frappe.module_links[module2] && frappe.module_links[module2].includes(doctype)) {
            frappe.breadcrumbs.set_doctype_module(doctype, module2);
          }
        }
      }
    }
    set_cur_list() {
      cur_list = frappe.views.list_view[this.page_name];
      if (cur_list && cur_list.doctype !== this.route[1]) {
        window.cur_list = null;
      }
    }
  };

  // frappe/public/js/frappe/list/list_view_select.js
  frappe.provide("frappe.views");
  frappe.views.ListViewSelect = class ListViewSelect {
    constructor(opts) {
      $.extend(this, opts);
      this.set_current_view();
      this.setup_views();
    }
    add_view_to_menu(view, action) {
      if (this.doctype == "File" && view == "List") {
        view = "File";
      }
      let $el = this.page.add_custom_menu_item(
        this.parent,
        this.label_map[view] || __(view),
        action,
        true,
        null,
        this.icon_map[view] || "list"
      );
      $el.parent().attr("data-view", view);
    }
    set_current_view() {
      this.current_view = "List";
      const route = frappe.get_route();
      const view_name = frappe.utils.to_title_case(route[2] || "");
      if (route.length > 2 && frappe.views.view_modes.includes(view_name)) {
        this.current_view = view_name;
        if (this.current_view === "Kanban") {
          this.kanban_board = route[3];
        } else if (this.current_view === "Inbox") {
          this.email_account = route[3];
        }
      }
    }
    set_route(view, calendar_name) {
      const route = [this.slug(), "view", view];
      if (calendar_name)
        route.push(calendar_name);
      let search_params = cur_list == null ? void 0 : cur_list.get_search_params();
      if (search_params) {
        frappe.route_options = Object.fromEntries(search_params);
      }
      frappe.set_route(route);
    }
    setup_views() {
      const views = {
        List: {
          condition: true,
          action: () => this.set_route("list")
        },
        Report: {
          condition: true,
          action: () => this.set_route("report"),
          current_view_handler: () => {
            const reports = this.get_reports();
            let default_action = {};
            if (frappe.get_route().length > 3) {
              default_action = {
                label: __("Report Builder"),
                action: () => frappe.set_route("report")
              };
            }
            this.setup_dropdown_in_navbar("Report", reports, default_action);
          }
        },
        Dashboard: {
          condition: true,
          action: () => this.set_route("dashboard")
        },
        Calendar: {
          condition: frappe.views.calendar[this.doctype],
          action: () => this.set_route("calendar", "default"),
          current_view_handler: () => {
            this.get_calendars().then((calendars) => {
              this.setup_dropdown_in_navbar("Calendar", calendars);
            });
          }
        },
        Gantt: {
          condition: frappe.views.calendar[this.doctype],
          action: () => this.set_route("gantt")
        },
        Inbox: {
          condition: this.doctype === "Communication" && frappe.boot.email_accounts.length,
          action: () => this.set_route("inbox"),
          current_view_handler: () => {
            const accounts = this.get_email_accounts();
            let default_action;
            if (has_common(frappe.user_roles, ["System Manager", "Administrator"])) {
              default_action = {
                label: __("New Email Account"),
                action: () => frappe.new_doc("Email Account")
              };
            }
            this.setup_dropdown_in_navbar("Inbox", accounts, default_action);
          }
        },
        Image: {
          condition: this.list_view.meta.image_field,
          action: () => this.set_route("image")
        },
        Tree: {
          condition: frappe.treeview_settings[this.doctype] || frappe.get_meta(this.doctype).is_tree,
          action: () => this.set_route("tree")
        },
        Kanban: {
          condition: this.doctype != "File",
          action: () => this.setup_kanban_boards(),
          current_view_handler: () => {
            frappe.views.KanbanView.get_kanbans(this.doctype).then(
              (kanbans) => this.setup_kanban_switcher(kanbans)
            );
          }
        },
        Map: {
          condition: this.list_view.settings.get_coords_method || this.list_view.meta.fields.find((i2) => i2.fieldname === "latitude") && this.list_view.meta.fields.find((i2) => i2.fieldname === "longitude") || this.list_view.meta.fields.find(
            (i2) => i2.fieldname === "location" && i2.fieldtype == "Geolocation"
          ),
          action: () => this.set_route("map")
        }
      };
      frappe.views.view_modes.forEach((view) => {
        if (this.current_view !== view && views[view].condition) {
          this.add_view_to_menu(view, views[view].action);
        }
        if (this.current_view == view) {
          views[view].current_view_handler && views[view].current_view_handler();
        }
      });
    }
    setup_dropdown_in_navbar(view, items, default_action) {
      let placeholder = __("Select {0}", [__(view)]);
      if (items && items.length) {
        items.map((item) => {
          this.page.add_inner_button(
            item.name,
            () => location.replace(item.route),
            placeholder
          );
        });
      }
      if (default_action && Object.keys(default_action).length) {
        this.page.add_inner_button(default_action.label, default_action.action, placeholder);
      }
    }
    setup_kanban_switcher(kanbans) {
      const kanban_switcher = this.page.add_custom_button_group(
        __("Select Kanban"),
        null,
        this.list_view.$filter_section
      );
      kanbans.map((k) => {
        this.page.add_custom_menu_item(
          kanban_switcher,
          k.name,
          () => this.set_route("kanban", k.name),
          false
        );
      });
      let perms = this.list_view.board_perms;
      let can_create = perms ? perms.create : true;
      if (can_create) {
        this.page.add_custom_menu_item(
          kanban_switcher,
          __("Create New Kanban Board"),
          () => frappe.views.KanbanView.show_kanban_dialog(this.doctype),
          true
        );
      }
    }
    get_page_name() {
      return frappe.utils.to_title_case(frappe.get_route().slice(-1)[0] || "");
    }
    get_reports() {
      let added = [];
      let reports_to_add = [];
      let add_reports = (reports2) => {
        reports2.map((r) => {
          if (!r.ref_doctype || r.ref_doctype == this.doctype) {
            const report_type = r.report_type === "Report Builder" ? `/desk/list/${r.ref_doctype}/report` : "/desk/query-report";
            const route = r.route || report_type + "/" + (r.title || r.name);
            if (added.indexOf(route) === -1) {
              added.push(route);
              reports_to_add.push({
                name: __(r.title || r.name),
                route
              });
            }
          }
        });
      };
      if (this.list_view.settings.reports) {
        add_reports(this.list_view.settings.reports);
      }
      var reports = Object.values(frappe.boot.user.all_reports).sort(
        (a, b) => a.title.localeCompare(b.title)
      ) || [];
      add_reports(reports);
      return reports_to_add;
    }
    setup_kanban_boards() {
      var _a3;
      function fetch_kanban_board(doctype) {
        frappe.db.get_value(
          "Kanban Board",
          { reference_doctype: doctype },
          "name",
          (board) => {
            if (!$.isEmptyObject(board)) {
              frappe.set_route("list", doctype, "kanban", board.name);
            } else {
              frappe.views.KanbanView.show_kanban_dialog(doctype);
            }
          }
        );
      }
      const last_opened_kanban = (_a3 = frappe.model.user_settings[this.doctype]["Kanban"]) == null ? void 0 : _a3.last_kanban_board;
      if (!last_opened_kanban) {
        fetch_kanban_board(this.doctype);
      } else {
        frappe.db.exists("Kanban Board", last_opened_kanban).then((exists) => {
          if (exists) {
            frappe.set_route("list", this.doctype, "kanban", last_opened_kanban);
          } else {
            fetch_kanban_board(this.doctype);
          }
        });
      }
    }
    get_calendars() {
      const doctype = this.doctype;
      let calendars = [];
      return frappe.db.get_list("Calendar View", {
        filters: {
          reference_doctype: doctype
        }
      }).then((result) => {
        if (!(result && Array.isArray(result) && result.length))
          return;
        if (frappe.views.calendar[this.doctype]) {
          calendars.push({
            name: "Default",
            route: `/desk/${this.slug()}/view/calendar/default`
          });
        }
        result.map((calendar) => {
          calendars.push({
            name: calendar.name,
            route: `/desk/${this.slug()}/view/calendar/${calendar.name}`
          });
        });
        return calendars;
      });
    }
    get_email_accounts() {
      let accounts_to_add = [];
      let accounts = frappe.boot.email_accounts;
      accounts.forEach((account) => {
        let email_account = account.email_id == "All Accounts" ? "All Accounts" : account.email_account;
        let route = `/desk/communication/view/inbox/${email_account}`;
        let display_name = ["All Accounts", "Sent Mail", "Spam", "Trash"].includes(
          account.email_id
        ) ? __(account.email_id) : account.email_account;
        accounts_to_add.push({
          name: display_name,
          route
        });
      });
      return accounts_to_add;
    }
    slug() {
      return frappe.router.slug(frappe.router.doctype_layout || this.doctype);
    }
  };

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/list/list_sidebar_stat.html
  frappe.templates["list_sidebar_stat"] = `{% if (stats.length) { %}
<div class="dropdown-search mb-1">
	<input type="text"
		placeholder="${__("Search")}"
		data-element="search"
		class="dropdown-search-input form-control input-xs"
	>
</div>
{% } %}
{% if (!stats.length) { %}
<li class="stat-no-records text-muted">{{ __("No records tagged.") }}</li>
{% } else {
	for (var i=0, l=stats.length; i < l; i++) {
		var stat_label = stats[i][0];
		var stat_count = stats[i][1];
%}
<li>
	<a class="stat-link dropdown-item flex justify-between group-by-item" data-label="{{ stat_label }}" data-value="{{ stat_label }}" data-field="_user_tags" href="#" onclick="return false;">

		<span class="stat-label">
			{% if (applied_filter == stat_label) { %}
				<span class="applied"> {{ frappe.utils.icon("tick", "xs") }} </span>
			{% } %}
			{{ __(stat_label) }}
		</span>
		<span>{{ stat_count }}</span>
	</a>
</li>
	{% }
} %}
`;

  // frappe/public/js/frappe/list/list_sidebar_group_by.js
  frappe.provide("frappe.views");
  frappe.views.ListGroupBy = class ListGroupBy {
    constructor(opts) {
      $.extend(this, opts);
      this.make_wrapper();
      this.user_settings = frappe.get_user_settings(this.doctype);
      this.group_by_fields = ["assigned_to", "owner"];
      if (this.user_settings.group_by_fields) {
        this.group_by_fields = this.group_by_fields.concat(this.user_settings.group_by_fields);
      }
      this.render_group_by_items();
      this.setup_dropdown();
      this.setup_filter_by();
    }
    make_wrapper() {
      this.$wrapper = this.sidebar.sidebar.find(".list-group-by");
      let html = `
			<div class="list-group-by-fields">
			</div>
			<div class="add-list-group-by sidebar-action">
				<a class="add-group-by">
					${__("Edit Filters")}
				</a>
			</div>
		`;
      this.$wrapper.html(html);
    }
    render_group_by_items() {
      let get_item_html = (fieldname) => {
        let label, fieldtype;
        if (fieldname === "assigned_to") {
          label = __("Assigned To");
        } else if (fieldname === "owner") {
          label = __("Created By");
        } else {
          label = frappe.meta.get_label(this.doctype, fieldname);
          let docfield = frappe.meta.get_docfield(this.doctype, fieldname);
          if (!docfield) {
            return;
          }
          fieldtype = docfield.fieldtype;
        }
        return `<div class="group-by-field list-link">
						<a class="btn btn-default btn-sm list-sidebar-button" data-toggle="dropdown"
						aria-haspopup="true" aria-expanded="false"
						data-label="${label}" data-fieldname="${fieldname}" data-fieldtype="${fieldtype}"
						href="#" onclick="return false;">
							<span class="ellipsis">${__(label)}</span>
							<span>${frappe.utils.icon("select", "xs")}</span>
						</a>
					<ul class="dropdown-menu group-by-dropdown" role="menu">
					</ul>
			</div>`;
      };
      let html = this.group_by_fields.map(get_item_html).join("");
      this.$wrapper.find(".list-group-by-fields").html(html);
    }
    setup_dropdown() {
      this.$wrapper.find(".group-by-field").on("show.bs.dropdown", (e) => {
        let $dropdown = $(e.currentTarget).find(".group-by-dropdown");
        this.set_loading_state($dropdown);
        let fieldname = $(e.currentTarget).find("a").attr("data-fieldname");
        let fieldtype = $(e.currentTarget).find("a").attr("data-fieldtype");
        this.get_group_by_count(fieldname).then((field_count_list) => {
          if (field_count_list.length) {
            let applied_filter = this.list_view.get_filter_value(
              fieldname == "assigned_to" ? "_assign" : fieldname
            );
            this.render_dropdown_items(
              field_count_list,
              fieldtype,
              $dropdown,
              applied_filter
            );
            this.setup_search($dropdown);
          } else {
            this.set_empty_state($dropdown);
          }
        });
      });
    }
    set_loading_state($dropdown) {
      $dropdown.html(`<li>
			<div class="empty-state group-by-loading">
				${__("Loading...")}
			</div>
		</li>`);
    }
    set_empty_state($dropdown) {
      $dropdown.html(
        `<div class="empty-state group-by-empty">
				${__("No filters found")}
			</div>`
      );
    }
    setup_search($dropdown) {
      frappe.utils.setup_search($dropdown, ".group-by-item", ".group-by-value", "data-name");
    }
    get_group_by_count(field) {
      let current_filters = this.list_view.get_filters_for_args();
      current_filters = current_filters.filter(
        (f_arr) => !f_arr.includes(field === "assigned_to" ? "_assign" : field)
      );
      let args = {
        doctype: this.doctype,
        current_filters,
        field
      };
      return frappe.call("frappe.desk.listview.get_group_by_count", args).then((r) => {
        let field_counts = r.message || [];
        field_counts = field_counts.filter((f) => f.count !== 0);
        let current_user = field_counts.find((f) => f.name === frappe.session.user);
        field_counts = field_counts.filter(
          (f) => !["Guest", "Administrator", frappe.session.user].includes(f.name)
        );
        if (current_user)
          field_counts.unshift(current_user);
        return field_counts;
      });
    }
    render_dropdown_items(fields, fieldtype, $dropdown, applied_filter) {
      let standard_html = `
			<div class="dropdown-search">
				<input type="text"
					placeholder="${__("Search")}"
					data-element="search"
					class="dropdown-search-input form-control input-xs"
				>
			</div>
		`;
      let applied_filter_html = "";
      let dropdown_items_html = "";
      fields.map((field) => {
        if (field.name === applied_filter) {
          applied_filter_html = this.get_dropdown_html(field, fieldtype, true);
        } else {
          dropdown_items_html += this.get_dropdown_html(field, fieldtype);
        }
      });
      let dropdown_html = standard_html + applied_filter_html + dropdown_items_html;
      $dropdown.toggleClass("has-selected", Boolean(applied_filter_html));
      $dropdown.html(dropdown_html);
    }
    get_dropdown_html(field, fieldtype, applied = false) {
      let label;
      if (field.name == null) {
        label = __("Not Set");
      } else if (field.name === frappe.session.user) {
        label = __("Me");
      } else if (fieldtype && fieldtype == "Check") {
        label = field.name == "0" ? __("No") : __("Yes");
      } else if (fieldtype && fieldtype == "Link" && field.title) {
        label = __(field.title);
      } else {
        label = __(field.name);
      }
      let value = field.name == null ? "" : encodeURIComponent(field.name);
      let applied_html = applied ? `<span class="applied"> ${frappe.utils.icon("tick", "xs")} </span>` : "";
      return `<div class="group-by-item ${applied ? "selected" : ""}" data-value="${value}">
			<a class="dropdown-item" href="#" onclick="return false;">
				${applied_html}
				<span class="group-by-value ellipsis" data-name="${field.name}">${label}</span>
				<span class="group-by-count">${field.count}</span>
			</a>
		</div>`;
    }
    setup_filter_by() {
      this.$wrapper.on("click", ".group-by-item", (e) => {
        let $target = $(e.currentTarget);
        let is_selected = $target.hasClass("selected");
        let fieldname = $target.parents(".group-by-field").find("a").data("fieldname");
        let value = typeof $target.data("value") === "string" ? decodeURIComponent($target.data("value").trim()) : $target.data("value");
        fieldname = fieldname === "assigned_to" ? "_assign" : fieldname;
        return this.list_view.filter_area.remove(fieldname).then(() => {
          if (is_selected)
            return;
          return this.apply_filter(fieldname, value);
        });
      });
    }
    apply_filter(fieldname, value) {
      let operator = "=";
      if (value === "") {
        operator = "is";
        value = "not set";
      }
      if (fieldname === "_assign") {
        operator = "like";
        value = `%${value}%`;
      }
      return this.list_view.filter_area.add(this.doctype, fieldname, operator, value);
    }
  };

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/list/list_view_permission_restrictions.html
  frappe.templates["list_view_permission_restrictions"] = `<table class="table table-bordered" style="margin: 0;">
	<thead>
		<th>{{ __("Field") }}</th>
		<th>{{ __("Value") }}</th>
	</thead>
	<tbody>
		{% for (let condition of condition_list ) { %}
			{% for (let key in condition) { %}
			<tr>
				<td>{{ __(key) }}</td>
				<td>{{ frappe.utils.comma_or(condition[key]) }}</td>
			</tr>
			{% } %}
		{% } %}
	</tbody>
</table>
`;

  // frappe/public/js/frappe/views/gantt/gantt_view.js
  frappe.provide("frappe.views");
  frappe.views.GanttView = class GanttView extends frappe.views.ListView {
    get view_name() {
      return "Gantt";
    }
    setup_defaults() {
      return super.setup_defaults().then(() => {
        this.page_title = this.page_title + " " + __("Gantt");
        this.calendar_settings = frappe.views.calendar[this.doctype] || {};
        if (typeof this.calendar_settings.gantt == "object") {
          Object.assign(this.calendar_settings, this.calendar_settings.gantt);
        }
        if (this.calendar_settings.order_by) {
          this.sort_by = this.calendar_settings.order_by;
          this.sort_order = "asc";
        } else {
          this.sort_by = this.view_user_settings.sort_by || this.calendar_settings.field_map.start;
          this.sort_order = this.view_user_settings.sort_order || "asc";
        }
      });
    }
    setup_view() {
    }
    prepare_data(data) {
      super.prepare_data(data);
      this.prepare_tasks();
    }
    prepare_tasks() {
      var me2 = this;
      var meta = this.meta;
      var field_map = this.calendar_settings.field_map;
      this.tasks = this.data.map(function(item) {
        var progress = 0;
        if (field_map.progress && $.isFunction(field_map.progress)) {
          progress = field_map.progress(item);
        } else if (field_map.progress) {
          progress = item[field_map.progress];
        }
        var label;
        if (meta.title_field) {
          label = item.progress ? __("{0} ({1}) - {2}%", [item[meta.title_field], item.name, item.progress]) : __("{0} ({1})", [item[meta.title_field], item.name]);
        } else {
          label = item[field_map.title];
        }
        var r = {
          start: item[field_map.start],
          end: item[field_map.end],
          name: label,
          id: item[field_map.id || "name"],
          doctype: me2.doctype,
          progress,
          dependencies: item.depends_on_tasks || ""
        };
        if (item.color && frappe.ui.color.validate_hex(item.color)) {
          r["custom_class"] = "color-" + item.color.substr(1);
        }
        if (item.is_milestone) {
          r["custom_class"] = "bar-milestone";
        }
        return r;
      });
    }
    render() {
      this.load_lib.then(() => {
        this.render_gantt();
      });
    }
    render_header() {
    }
    render_gantt() {
      const me2 = this;
      const gantt_view_mode = this.view_user_settings.gantt_view_mode || "Day";
      const field_map = this.calendar_settings.field_map;
      const date_format = "YYYY-MM-DD";
      this.$result.empty();
      this.$result.addClass("gantt-modern");
      this.gantt = new Gantt(this.$result[0], this.tasks, {
        bar_height: 35,
        bar_corner_radius: 4,
        resize_handle_width: 8,
        resize_handle_height: 28,
        resize_handle_corner_radius: 3,
        resize_handle_offset: 4,
        view_mode: gantt_view_mode,
        date_format: "YYYY-MM-DD",
        on_click: (task) => {
          frappe.set_route("Form", task.doctype, task.id);
        },
        on_date_change: (task, start2, end2) => {
          if (!me2.can_write)
            return;
          frappe.db.set_value(task.doctype, task.id, {
            [field_map.start]: moment(start2).format(date_format),
            [field_map.end]: moment(end2).format(date_format)
          });
        },
        on_progress_change: (task, progress) => {
          if (!me2.can_write)
            return;
          var progress_fieldname = "progress";
          if ($.isFunction(field_map.progress)) {
            progress_fieldname = null;
          } else if (field_map.progress) {
            progress_fieldname = field_map.progress;
          }
          if (progress_fieldname) {
            frappe.db.set_value(task.doctype, task.id, {
              [progress_fieldname]: parseInt(progress)
            });
          }
        },
        on_view_change: (mode) => {
          me2.save_view_user_settings({
            gantt_view_mode: mode
          });
        },
        custom_popup_html: (task) => {
          var item = me2.get_item(task.id);
          var html = `<div class="title">${task.name}</div>
					<div class="subtitle">${moment(task._start).format("MMM D")} - ${moment(task._end).format(
            "MMM D"
          )}</div>`;
          var custom = me2.settings.gantt_custom_popup_html;
          if (custom && $.isFunction(custom)) {
            var ganttobj = task;
            html = custom(ganttobj, item);
          }
          return '<div class="details-container">' + html + "</div>";
        }
      });
      this.setup_view_mode_buttons();
      this.set_colors();
    }
    setup_view_mode_buttons() {
      let $btn_group = this.$paging_area.find(".gantt-view-mode");
      if ($btn_group.length > 0)
        return;
      const view_modes = this.gantt.options.view_modes || [];
      const active_class = (view_mode) => this.gantt.view_is(view_mode) ? "btn-info" : "";
      const html = `<div class="btn-group gantt-view-mode mx-2">
				${view_modes.map(
        (value) => `<button type="button"
						class="btn btn-default btn-sm btn-view-mode ${active_class(value)}"
						data-value="${value}">
						${__(value)}
					</button>`
      ).join("")}
			</div>`;
      this.$paging_area.find(".level-left").append(html);
      const change_view_mode = (value) => setTimeout(() => this.gantt.change_view_mode(value), 0);
      this.$paging_area.on("click", ".btn-view-mode", (e) => {
        const $btn = $(e.currentTarget);
        this.$paging_area.find(".btn-view-mode").removeClass("btn-info");
        $btn.addClass("btn-info");
        const value = $btn.data().value;
        change_view_mode(value);
      });
    }
    set_colors() {
      const classes = this.tasks.map((t) => t.custom_class).filter((c) => c && c.startsWith("color-"));
      let style = classes.map((c) => {
        const class_name = c.replace("#", "");
        const bar_color = "#" + c.substr(6);
        const progress_color = frappe.ui.color.get_contrast_color(bar_color);
        return `
				.gantt .bar-wrapper.${class_name} .bar {
					fill: ${bar_color};
				}
				.gantt .bar-wrapper.${class_name} .bar-progress {
					fill: ${progress_color};
				}
			`;
      }).join("");
      style = `<style>${style}</style>`;
      this.$result.prepend(style);
    }
    get_item(name) {
      return this.data.find((item) => item.name === name);
    }
    get required_libs() {
      return [
        "assets/frappe/node_modules/frappe-gantt/dist/frappe-gantt.css",
        "assets/frappe/node_modules/frappe-gantt/dist/frappe-gantt.min.js"
      ];
    }
  };

  // frappe/public/js/frappe/views/calendar/calendar.js
  frappe.provide("frappe.views.calendar");
  frappe.provide("frappe.views.calendars");
  frappe.views.CalendarView = class CalendarView extends frappe.views.ListView {
    static load_last_view() {
      const route = frappe.get_route();
      if (route.length === 3) {
        const doctype = route[1];
        const user_settings = frappe.get_user_settings(doctype)["Calendar"] || {};
        route.push(user_settings.last_calendar || "default");
        frappe.route_flags.replace_route = true;
        frappe.set_route(route);
        return true;
      } else {
        return false;
      }
    }
    toggle_result_area() {
    }
    get view_name() {
      return "Calendar";
    }
    setup_defaults() {
      return super.setup_defaults().then(() => {
        this.page_title = __("{0} Calendar", [this.page_title]);
        this.calendar_settings = frappe.views.Calendar[this.doctype] || {};
        this.calendar_name = frappe.get_route()[3];
      });
    }
    setup_page() {
      this.hide_page_form = true;
      super.setup_page();
    }
    setup_view() {
    }
    before_render() {
      super.before_render();
      this.save_view_user_settings({
        last_calendar: this.calendar_name
      });
    }
    render() {
      if (this.calendar) {
        this.calendar.refresh();
        return;
      }
      this.load_lib.then(() => this.get_calendar_preferences()).then((options) => {
        this.calendar = new frappe.views.Calendar(options);
      });
    }
    get_calendar_preferences() {
      const options = {
        doctype: this.doctype,
        parent: this.$result,
        page: this.page,
        list_view: this
      };
      const calendar_name = this.calendar_name;
      return new Promise((resolve) => {
        if (calendar_name === "default") {
          Object.assign(options, frappe.views.calendar[this.doctype]);
          resolve(options);
        } else {
          frappe.model.with_doc("Calendar View", calendar_name, () => {
            const doc = frappe.get_doc("Calendar View", calendar_name);
            if (!doc) {
              frappe.show_alert(
                __("{0} is not a valid Calendar. Redirecting to default Calendar.", [
                  calendar_name.bold()
                ])
              );
              frappe.set_route("List", this.doctype, "Calendar", "default");
              return;
            }
            Object.assign(options, {
              field_map: {
                id: "name",
                start: doc.start_date_field,
                end: doc.end_date_field,
                title: doc.subject_field,
                allDay: doc.all_day ? 1 : 0
              }
            });
            resolve(options);
          });
        }
      });
    }
    get required_libs() {
      return "calendar.bundle.js";
    }
  };
  frappe.views.Calendar = class Calendar {
    constructor(options) {
      $.extend(this, options);
      this.field_map = this.field_map || {
        id: "name",
        start: "start",
        end: "end",
        allDay: "all_day",
        convertToUserTz: "convert_to_user_tz"
      };
      this.color_map = {
        danger: "red",
        success: "green",
        warning: "orange",
        default: "blue"
      };
      this.get_default_options();
    }
    get_default_options() {
      return new Promise((resolve) => {
        let initialView = localStorage.getItem("cal_initialView");
        let weekends = localStorage.getItem("cal_weekends");
        let defaults = {
          initialView: initialView ? initialView : "dayGridMonth",
          weekends: weekends ? weekends : true
        };
        resolve(defaults);
      }).then((defaults) => {
        this.make_page();
        this.setup_options(defaults);
        this.make();
        this.setup_view_mode_button(defaults);
        this.bind();
      });
    }
    make_page() {
      var me2 = this;
      me2.page.clear_user_actions();
      $.each(frappe.boot.calendars, function(i2, doctype) {
        if (frappe.model.can_read(doctype)) {
          me2.page.add_menu_item(__(doctype), function() {
            frappe.set_route("List", doctype, "Calendar");
          });
        }
      });
      $(this.parent).on("show", function() {
        me2.$cal.fullCalendar.refetchEvents();
      });
    }
    make() {
      this.$wrapper = this.parent;
      this.$cal = $("<div id='fc-calendar-wrapper'>").appendTo(this.$wrapper);
      this.footnote_area = frappe.utils.set_footnote(
        this.footnote_area,
        this.$wrapper,
        __("Select or drag across time slots to create a new event.")
      );
      this.footnote_area.addClass("px-4 pb-4").css({
        "border-top": "0px"
      });
      this.fullCalendar = new frappe.FullCalendar(this.$cal[0], this.cal_options);
      this.fullCalendar.render();
      this.set_css();
    }
    setup_view_mode_button(defaults) {
      var me2 = this;
      $(me2.footnote_area).find(".btn-weekend").detach();
      let btnTitle = defaults.weekends ? __("Hide Weekends") : __("Show Weekends");
      const btn = `<button class="btn btn-default btn-xs btn-weekend">${btnTitle}</button>`;
      me2.footnote_area.append(btn);
    }
    set_localStorage_option(option, value) {
      localStorage.removeItem(option);
      localStorage.setItem(option, value);
    }
    bind() {
      const me2 = this;
      let btn_group = me2.$wrapper.find(".fc-button-group");
      btn_group.on("click", ".btn", function() {
        let value = $(this).hasClass("fc-timeGridWeek-button") ? "timeGridWeek" : $(this).hasClass("fc-timeGridDay-button") ? "timeGridDay" : "dayGridMonth";
        me2.set_localStorage_option("cal_initialView", value);
      });
      me2.$wrapper.on("click", ".btn-weekend", function() {
        me2.cal_options.weekends = !me2.cal_options.weekends;
        me2.fullCalendar.setOption("weekends", me2.cal_options.weekends);
        me2.set_localStorage_option("cal_weekends", me2.cal_options.weekends);
        me2.set_css();
        me2.setup_view_mode_button(me2.cal_options);
      });
    }
    set_css() {
      const viewButtons = ".fc-dayGridMonth-button, .fc-timeGridWeek-button, .fc-timeGridDay-button, .fc-today-button";
      const fcViewButtonClasses = "fc-button fc-button-primary fc-button-active";
      this.$wrapper.find("button.fc-button").removeClass(fcViewButtonClasses).addClass("btn btn-default");
      this.$wrapper.find(viewButtons).wrapAll('<div class="btn-group" />');
      this.$wrapper.find(`.fc-prev-button span`).attr("class", "").html(frappe.utils.icon("left"));
      this.$wrapper.find(`.fc-next-button span`).attr("class", "").html(frappe.utils.icon("right"));
      if (this.$wrapper.find(".fc-today-button svg").length == 0)
        this.$wrapper.find(".fc-today-button").prepend(frappe.utils.icon("today"));
      var btn_group = this.$wrapper.find(".fc-button-group");
      btn_group.find(".fc-button-active").addClass("active");
      btn_group.find(".btn").on("click", function() {
        btn_group.find(viewButtons).removeClass(`active ${fcViewButtonClasses}`).addClass("btn btn-default");
        $(this).addClass("active");
      });
    }
    get_system_datetime(date) {
      return frappe.datetime.convert_to_system_tz(date, true);
    }
    setup_options(defaults) {
      var me2 = this;
      defaults.meridiem = "false";
      this.cal_options = {
        plugins: frappe.FullCalendar.Plugins,
        initialView: defaults.initialView || "dayGridMonth",
        locale: frappe.boot.lang,
        eventTimeFormat: {
          hour: "numeric",
          minute: "2-digit",
          hour12: true
        },
        firstDay: frappe.datetime.get_first_day_of_the_week_index(),
        eventDisplay: "block",
        headerToolbar: {
          left: "prev,title,next",
          center: "",
          right: "today,dayGridMonth,timeGridWeek,timeGridDay"
        },
        editable: true,
        droppable: true,
        selectable: true,
        selectMirror: true,
        forceEventDuration: true,
        displayEventTime: true,
        weekends: defaults.weekends,
        nowIndicator: true,
        themeSystem: null,
        buttonText: {
          today: __("Today"),
          month: __("Month"),
          week: __("Week"),
          day: __("Day")
        },
        events: function(info, successCallback, failureCallback) {
          return frappe.call({
            method: me2.get_events_method || "frappe.desk.calendar.get_events",
            type: "GET",
            args: me2.get_args(info.start, info.end),
            callback: function(r) {
              var events = r.message || [];
              events = me2.prepare_events(events);
              successCallback(events);
            }
          });
        },
        displayEventEnd: true,
        eventClick: function(info) {
          var doctype = info.doctype || me2.doctype;
          if (frappe.model.can_read(doctype)) {
            frappe.set_route("Form", doctype, info.event.id);
          }
        },
        eventDrop: function(info) {
          me2.update_event(info.event, info.revert);
        },
        eventResize: function(info) {
          me2.update_event(info.event, info.revert);
        },
        select: function(info) {
          const seconds = info.end - info.start;
          const allDay = seconds === 864e5;
          if (info.view.type === "dayGridMonth" && allDay) {
            return;
          }
          var event2 = frappe.model.get_new_doc(me2.doctype);
          event2[me2.field_map.start] = me2.get_system_datetime(info.start);
          if (me2.field_map.end)
            event2[me2.field_map.end] = me2.get_system_datetime(info.end);
          if (seconds >= 864e5) {
            if (allDay) {
              event2[me2.field_map.allDay] = 1;
            }
            event2[me2.field_map.end] = me2.get_system_datetime(info.end - 1);
          }
          frappe.set_route("Form", me2.doctype, event2.name);
        },
        dateClick: function(info) {
          if (info.view.type === "dayGridMonth") {
            const $date_cell = $(
              "td[data-date=" + info.date.toISOString().slice(0, 10) + "]"
            );
            if ($date_cell.hasClass("date-clicked")) {
              me2.fullCalendar.changeView("timeGridDay", info.date);
              me2.$wrapper.find(".date-clicked").removeClass("date-clicked");
              me2.$wrapper.find(".fc-month-button").removeClass("active");
              me2.$wrapper.find(".fc-agendaDay-button").addClass("active");
            }
            me2.$wrapper.find(".date-clicked").removeClass("date-clicked");
            $date_cell.addClass("date-clicked");
            $("#fc-calendar-wrapper").find("button.fc-button").removeClass("fc-button fc-button-primary fc-button-active").addClass("btn btn-default");
          }
          return false;
        }
      };
      if (this.options) {
        $.extend(this.cal_options, this.options);
      }
    }
    get_args(start2, end2) {
      var args = {
        doctype: this.doctype,
        start: this.get_system_datetime(start2),
        end: this.get_system_datetime(end2),
        fields: this.fields,
        filters: this.list_view.filter_area.get(),
        field_map: this.field_map
      };
      return args;
    }
    refresh() {
      this.fullCalendar.refetchEvents();
    }
    prepare_events(events) {
      var me2 = this;
      return (events || []).map((d) => {
        d.id = d.name;
        d.editable = frappe.model.can_write(d.doctype || me2.doctype);
        if (d.docstatus && d.docstatus > 0) {
          d.editable = false;
        }
        $.each(me2.field_map, function(target, source) {
          d[target] = d[source];
        });
        if (typeof d.allDay === "undefined") {
          d.allDay = me2.field_map.allDay;
        }
        if (!me2.field_map.convertToUserTz)
          d.convertToUserTz = 1;
        if (d.convertToUserTz) {
          d.start = frappe.datetime.convert_to_user_tz(d.start);
          d.end = frappe.datetime.convert_to_user_tz(d.end);
        }
        if (!frappe.datetime.validate(d.start) && d.end) {
          d.start = frappe.datetime.add_days(d.end, -1);
        }
        if (d.start && !frappe.datetime.validate(d.end)) {
          d.end = frappe.datetime.add_days(d.start, 1);
        }
        me2.prepare_colors(d);
        d.title = frappe.utils.html2text(d.title);
        return d;
      });
    }
    prepare_colors(d) {
      let color, color_name;
      if (this.get_css_class) {
        color_name = this.get_css_class(d);
        color_name = this.color_map[color_name] || color_name || "blue";
        if (color_name.startsWith("#")) {
          color_name = frappe.ui.color.validate_hex(color_name) ? color_name : "blue";
        }
        d.backgroundColor = frappe.ui.color.get(color_name, "extra-light");
        d.textColor = frappe.ui.color.get(color_name, "dark");
      } else {
        color = d.color;
        if (!frappe.ui.color.validate_hex(color) || !color) {
          color = frappe.ui.color.get("blue", "extra-light");
        }
        d.backgroundColor = color;
        d.textColor = frappe.ui.color.get_contrast_color(color);
      }
      return d;
    }
    update_event(event2, revertFunc) {
      var me2 = this;
      frappe.model.remove_from_locals(me2.doctype, event2.id);
      return frappe.call({
        method: me2.update_event_method || "frappe.desk.calendar.update_event",
        args: me2.get_update_args(event2),
        callback: function(r) {
          if (r.exc) {
            frappe.show_alert(__("Unable to update event"));
            revertFunc();
          }
        },
        error: function() {
          revertFunc();
        }
      });
    }
    get_update_args(event2) {
      var me2 = this;
      var args = {
        name: event2.id
      };
      args[this.field_map.start] = me2.get_system_datetime(event2.start);
      if (this.field_map.allDay) {
        args[this.field_map.allDay] = event2.end - event2.start === 864e5 ? 1 : 0;
      }
      if (this.field_map.end) {
        if (!event2.end) {
          event2.end = event2.start.add(1, "hour");
        }
        args[this.field_map.end] = me2.get_system_datetime(event2.end);
        if (args[this.field_map.allDay]) {
          args[this.field_map.end] = me2.get_system_datetime(new Date(event2.end - 1e3));
        }
      }
      args.doctype = event2.doctype || this.doctype;
      return { args, field_map: this.field_map };
    }
  };

  // frappe/public/js/frappe/views/dashboard/dashboard_view.js
  frappe.provide("frappe.views");
  var _a;
  frappe.views.DashboardView = (_a = class extends frappe.views.ListView {
    get view_name() {
      return "Dashboard";
    }
    setup_defaults() {
      return super.setup_defaults().then(() => {
        this.page_title = __("{0} Dashboard", [__(this.doctype)]);
        this.dashboard_settings = frappe.get_user_settings(this.doctype)["dashboard_settings"] || null;
      });
    }
    render() {
    }
    setup_page() {
      this.hide_page_form = true;
      this.hide_filters = true;
      this.hide_sort_selector = true;
      super.setup_page();
    }
    setup_view() {
      if (this.chart_group || this.number_card_group) {
        return;
      }
      this.setup_dashboard_page();
      this.setup_dashboard_customization();
      this.make_dashboard();
    }
    setup_dashboard_customization() {
      this.page.add_menu_item(__("Customize Dashboard"), () => this.customize());
      this.page.add_menu_item(
        __("Reset Dashboard Customizations"),
        () => this.reset_dashboard_customization()
      );
      this.add_customization_buttons();
    }
    setup_dashboard_page() {
      const chart_wrapper_html = `<div class="dashboard-view"></div>`;
      this.$frappe_list.html(chart_wrapper_html);
      this.page.clear_secondary_action();
      this.$dashboard_page = this.$page.find(".layout-main-section-wrapper").addClass("dashboard-page");
      this.page.main.removeClass("frappe-card");
      this.$dashboard_wrapper = this.$page.find(".dashboard-view");
      this.$chart_header = this.$page.find(".dashboard-header");
      frappe.utils.bind_actions_with_object(this.$dashboard_page, this);
    }
    add_customization_buttons() {
      this.save_customizations_button = this.page.add_button(
        __("Save Customizations"),
        () => {
          this.save_dashboard_customization();
          this.page.standard_actions.show();
        },
        { btn_class: "btn-primary" }
      );
      this.discard_customizations_button = this.page.add_button(__("Discard"), () => {
        this.discard_dashboard_customization();
        this.page.standard_actions.show();
      });
      this.toggle_customization_buttons(false);
    }
    set_primary_action() {
    }
    toggle_customization_buttons(show) {
      this.save_customizations_button.toggle(show);
      this.discard_customizations_button.toggle(show);
    }
    make_dashboard() {
      if (this.dashboard_settings) {
        this.charts = this.dashboard_settings.charts;
        this.number_cards = this.dashboard_settings.number_cards;
        this.render_dashboard();
      } else {
        frappe.run_serially([
          () => this.fetch_dashboard_items(
            "Dashboard Chart",
            {
              chart_type: ["in", ["Count", "Sum", "Group By"]],
              document_type: this.doctype,
              is_public: 1
            },
            "charts"
          ),
          () => this.fetch_dashboard_items(
            "Number Card",
            {
              document_type: this.doctype,
              is_public: 1
            },
            "number_cards"
          ),
          () => this.render_dashboard()
        ]);
      }
    }
    render_dashboard() {
      this.$dashboard_wrapper.empty();
      frappe.dashboard_utils.get_dashboard_settings().then((settings) => {
        this.dashboard_chart_settings = settings.chart_config ? JSON.parse(settings.chart_config) : {};
        this.charts.map((chart) => {
          chart.label = chart.chart_name;
          chart.chart_settings = this.dashboard_chart_settings[chart.chart_name] || {};
        });
        this.render_dashboard_charts();
      });
      this.render_number_cards();
      if (!this.charts.length && !this.number_cards.length) {
        this.render_empty_state();
      }
    }
    fetch_dashboard_items(doctype, filters, obj_name) {
      return frappe.db.get_list(doctype, {
        filters,
        fields: ["*"]
      }).then((items) => {
        this[obj_name] = items;
      });
    }
    render_number_cards() {
      this.number_card_group = new frappe.widget.WidgetGroup({
        container: this.$dashboard_wrapper,
        type: "number_card",
        columns: 3,
        options: {
          allow_sorting: true,
          allow_create: true,
          allow_delete: true,
          allow_hiding: true
        },
        default_values: { doctype: this.doctype },
        widgets: this.number_cards || [],
        in_customize_mode: this.in_customize_mode || false
      });
      this.in_customize_mode && this.number_card_group.customize();
    }
    render_dashboard_charts() {
      this.chart_group = new frappe.widget.WidgetGroup({
        container: this.$dashboard_wrapper,
        type: "chart",
        columns: 2,
        height: 240,
        options: {
          allow_sorting: true,
          allow_create: true,
          allow_delete: true,
          allow_hiding: true,
          allow_resize: true
        },
        custom_dialog: () => this.show_add_chart_dialog(),
        widgets: this.charts,
        in_customize_mode: this.in_customize_mode || false
      });
      this.in_customize_mode && this.chart_group.customize();
      this.chart_group.container.find(".widget-group-head").hide();
    }
    render_empty_state() {
      const no_result_message_html = `<p>${__(
        "You haven't added any Dashboard Charts or Number Cards yet."
      )}
			<br>${__("Click On Customize to add your first widget")}</p>`;
      const customize_button = `<p><button class="btn btn-primary btn-sm" data-action="customize">
				${__("Customize")}
			</button></p>`;
      const empty_state_html = `<div class="msg-box no-border empty-dashboard">
			<div>
				<svg class="icon icon-xl" style="stroke: var(--text-light);">
					<use href="#icon-small-file"></use>
				</svg>
			</div>
			${no_result_message_html}
			${customize_button}
		</div>`;
      this.$dashboard_wrapper.append(empty_state_html);
      this.$empty_state = this.$dashboard_wrapper.find(".empty-dashboard");
    }
    customize() {
      if (this.in_customize_mode) {
        return;
      }
      this.page.standard_actions.hide();
      if (this.$empty_state) {
        this.$empty_state.remove();
      }
      this.toggle_customize(true);
      this.in_customize_mode = true;
      this.chart_group.customize();
      this.number_card_group.customize();
    }
    get_widgets_to_save(widget_group) {
      const config = widget_group.get_widget_config();
      let widgets = [];
      config.order.map((widget_name) => {
        widgets.push(config.widgets[widget_name]);
      });
      return this.remove_duplicates(widgets);
    }
    save_dashboard_customization() {
      this.toggle_customize(false);
      const charts = this.get_widgets_to_save(this.chart_group);
      const number_cards = this.get_widgets_to_save(this.number_card_group);
      this.dashboard_settings = {
        charts,
        number_cards
      };
      frappe.model.user_settings.save(
        this.doctype,
        "dashboard_settings",
        this.dashboard_settings
      );
      this.make_dashboard();
    }
    discard_dashboard_customization() {
      this.dashboard_settings = frappe.get_user_settings(this.doctype)["dashboard_settings"] || null;
      this.toggle_customize(false);
      this.render_dashboard();
    }
    reset_dashboard_customization() {
      frappe.confirm(__("Are you sure you want to reset all customizations?"), () => {
        this.dashboard_settings = null;
        frappe.model.user_settings.save(this.doctype, "dashboard_settings", this.dashboard_settings).then(() => this.make_dashboard());
        this.toggle_customize(false);
      });
    }
    toggle_customize(show) {
      this.toggle_customization_buttons(show);
      this.in_customize_mode = show;
    }
    show_add_chart_dialog() {
      let fields = this.get_field_options();
      const dialog = new frappe.ui.Dialog({
        title: __("Add a {0} Chart", [__(this.doctype)]),
        fields: [
          {
            fieldname: "new_or_existing",
            fieldtype: "Select",
            label: "Choose an existing chart or create a new chart",
            options: ["New Chart", "Existing Chart"],
            reqd: 1
          },
          {
            label: "Chart",
            fieldname: "chart",
            fieldtype: "Link",
            get_query: () => {
              return {
                query: "frappe.desk.doctype.dashboard_chart.dashboard_chart.get_charts_for_user",
                filters: {
                  document_type: this.doctype
                }
              };
            },
            options: "Dashboard Chart",
            depends_on: 'eval: doc.new_or_existing == "Existing Chart"'
          },
          {
            fieldname: "sb_2",
            fieldtype: "Section Break",
            depends_on: 'eval: doc.new_or_existing == "New Chart"'
          },
          {
            label: "Chart Label",
            fieldname: "label",
            fieldtype: "Data",
            mandatory_depends_on: 'eval: doc.new_or_existing == "New Chart"'
          },
          {
            fieldname: "cb_1",
            fieldtype: "Column Break"
          },
          {
            label: "Chart Type",
            fieldname: "chart_type",
            fieldtype: "Select",
            options: ["Time Series", "Group By"],
            mandatory_depends_on: 'eval: doc.new_or_existing == "New Chart"'
          },
          {
            fieldname: "sb_2",
            fieldtype: "Section Break",
            label: "Chart Config",
            depends_on: 'eval: doc.chart_type == "Time Series" && doc.new_or_existing == "New Chart"'
          },
          {
            label: "Function",
            fieldname: "chart_function",
            fieldtype: "Select",
            options: ["Count", "Sum", "Average"],
            default: "Count"
          },
          {
            label: "Timespan",
            fieldtype: "Select",
            fieldname: "timespan",
            depends_on: 'eval: doc.chart_type == "Time Series"',
            options: ["Last Year", "Last Quarter", "Last Month", "Last Week"],
            default: "Last Year"
          },
          {
            fieldname: "cb_2",
            fieldtype: "Column Break"
          },
          {
            label: "Value Based On",
            fieldtype: "Select",
            fieldname: "based_on",
            options: fields.value_fields,
            depends_on: 'eval: doc.chart_function=="Sum"'
          },
          {
            label: "Time Series Based On",
            fieldtype: "Select",
            fieldname: "based_on",
            options: fields.date_fields,
            mandatory_depends_on: 'eval: doc.chart_type == "Time Series"'
          },
          {
            label: "Time Interval",
            fieldname: "time_interval",
            fieldtype: "Select",
            depends_on: 'eval: doc.chart_type == "Time Series"',
            options: ["Yearly", "Quarterly", "Monthly", "Weekly", "Daily"],
            default: "Monthly"
          },
          {
            fieldname: "sb_2",
            fieldtype: "Section Break",
            label: "Chart Config",
            depends_on: 'eval: doc.chart_type == "Group By" && doc.new_or_existing == "New Chart"'
          },
          {
            label: "Group By Type",
            fieldname: "group_by_type",
            fieldtype: "Select",
            options: ["Count", "Sum", "Average"],
            default: "Count"
          },
          {
            label: "Aggregate Function Based On",
            fieldtype: "Select",
            fieldname: "aggregate_function_based_on",
            options: fields.aggregate_function_fields,
            depends_on: 'eval: ["Sum", "Average"].includes(doc.group_by_type)'
          },
          {
            fieldname: "cb_2",
            fieldtype: "Column Break"
          },
          {
            label: "Group By Based On",
            fieldtype: "Select",
            fieldname: "group_by_based_on",
            options: fields.group_by_fields,
            default: "Last Year"
          },
          {
            label: "Number of Groups",
            fieldtype: "Int",
            fieldname: "number_of_groups",
            default: 0
          },
          {
            fieldname: "sb_3",
            fieldtype: "Section Break",
            depends_on: 'eval: doc.new_or_existing == "New Chart"'
          },
          {
            label: "Chart Type",
            fieldname: "type",
            fieldtype: "Select",
            options: ["Line", "Bar", "Percentage", "Pie"],
            depends_on: 'eval: doc.new_or_existing == "New Chart"'
          },
          {
            fieldname: "cb_1",
            fieldtype: "Column Break"
          },
          {
            label: "Chart Color",
            fieldname: "color",
            fieldtype: "Color",
            depends_on: 'eval: doc.new_or_existing == "New Chart"'
          }
        ],
        primary_action_label: __("Add"),
        primary_action: (values) => {
          let chart = values;
          if (chart.new_or_existing == "New Chart") {
            chart.chart_name = chart.label;
            chart.chart_type = chart.chart_type == "Time Series" ? chart.chart_function : chart.chart_type;
            chart.document_type = this.doctype;
            chart.filters_json = "[]";
            frappe.xcall(
              "frappe.desk.doctype.dashboard_chart.dashboard_chart.create_dashboard_chart",
              { args: chart }
            ).then((doc) => {
              this.chart_group.new_widget.on_create({
                chart_name: doc.chart_name,
                name: doc.chart_name,
                label: chart.label
              });
            });
          } else {
            this.chart_group.new_widget.on_create({
              chart_name: chart.chart,
              label: chart.chart,
              name: chart.chart
            });
          }
          dialog.hide();
        }
      });
      dialog.show();
    }
    get_field_options() {
      let date_fields = [
        { label: __("Created On"), value: "creation" },
        { label: __("Last Modified On"), value: "modified" }
      ];
      let value_fields = [];
      let group_by_fields = [];
      let aggregate_function_fields = [];
      frappe.get_meta(this.doctype).fields.map((df) => {
        if (["Date", "Datetime"].includes(df.fieldtype)) {
          date_fields.push({ label: df.label, value: df.fieldname });
        }
        if (frappe.model.numeric_fieldtypes.includes(df.fieldtype)) {
          if (df.fieldtype == "Currency") {
            if (!df.options || df.options !== "Company:company:default_currency") {
              return;
            }
          }
          value_fields.push({ label: df.label, value: df.fieldname });
          aggregate_function_fields.push({ label: df.label, value: df.fieldname });
        }
        if (["Link", "Select"].includes(df.fieldtype)) {
          group_by_fields.push({ label: df.label, value: df.fieldname });
        }
      });
      return {
        date_fields,
        value_fields,
        group_by_fields,
        aggregate_function_fields
      };
    }
    remove_duplicates(items) {
      return items.filter((item, index) => items.indexOf(item) === index);
    }
  }, __publicField(_a, "no_sidebar", true), _a);

  // frappe/public/js/frappe/views/image/image_view.js
  frappe.provide("frappe.views");
  frappe.views.ImageView = class ImageView extends frappe.views.ListView {
    get view_name() {
      return "Image";
    }
    setup_defaults() {
      return super.setup_defaults().then(() => {
        this.page_title = this.page_title + " " + __("Images");
      });
    }
    setup_view() {
      this.setup_columns();
      this.setup_check_events();
      this.setup_like();
    }
    set_fields() {
      this.fields = [
        "name",
        ...this.get_fields_in_list_view().map((el) => el.fieldname),
        this.meta.title_field,
        this.meta.image_field,
        "_liked_by"
      ];
    }
    prepare_data(data) {
      super.prepare_data(data);
      this.items = this.data.map((d) => {
        d._image_url = this.get_image_url(d);
        return d;
      });
    }
    render() {
      this.load_lib.then(() => {
        this.get_attached_images().then(() => {
          this.render_image_view();
          if (!this.gallery) {
            this.setup_gallery();
          } else {
            this.gallery.prepare_pswp_items(this.items, this.images_map);
          }
        });
      });
    }
    render_image_view() {
      var html = this.items.map(this.item_html.bind(this)).join("");
      this.$page.find(".layout-main-section-wrapper").addClass("image-view");
      this.$result.html(`
			<div class="image-view-container">
				${html}
			</div>
		`);
      this.render_count();
    }
    item_details_html(item) {
      let info_fields = this.get_fields_in_list_view().map((el) => el.fieldname) || [];
      const title_field = this.meta.title_field || "name";
      info_fields = info_fields.filter((field) => field !== title_field);
      let info_html = `<div><ul class="list-unstyled image-view-info">`;
      let set = false;
      info_fields.forEach((field, index) => {
        if (item[field] && !set) {
          let value = frappe.utils.escape_html(__(item[field]));
          if (index == 0)
            info_html += `<li>${value}</li>`;
          else
            info_html += `<li class="text-muted">${value}</li>`;
          set = true;
        }
      });
      info_html += `</ul></div>`;
      return info_html;
    }
    item_html(item) {
      item._name = encodeURI(item.name);
      const encoded_name = item._name;
      const title = strip_html(item[this.meta.title_field || "name"]);
      const escaped_title = frappe.utils.escape_html(title);
      const _class = !item._image_url ? "no-image" : "";
      const _html = item._image_url ? `<img data-name="${encoded_name}" src="${frappe.utils.escape_html(
        item._image_url
      )}" alt="${title}">` : `<span class="placeholder-text">
				${frappe.get_abbr(title)}
			</span>`;
      let details = this.item_details_html(item);
      const expand_button_html = item._image_url ? `<div class="zoom-view" data-name="${encoded_name}">
				${frappe.utils.icon("expand", "xs")}
			</div>` : "";
      return `
			<div class="image-view-item ellipsis">
				<div class="image-view-header">
					<div>
						<input class="level-item list-row-checkbox hidden-xs"
							type="checkbox" data-name="${escape(item.name)}">
						${this.get_like_html(item)}
					</div>
				</span>
				</div>
				<div class="image-view-body ${_class}">
					<a data-name="${encoded_name}"
						title="${encoded_name}"
						href="${this.get_form_link(item)}"
					>
						<div class="image-field"
							data-name="${encoded_name}"
						>
							${_html}
						</div>
					</a>
					${expand_button_html}
				</div>
				<div class="image-view-footer">
					<div class="image-title">
						<span class="ellipsis" title="${escaped_title}">
							<a class="ellipsis" href="${this.get_form_link(item)}"
								title="${escaped_title}" data-doctype="${this.doctype}" data-name="${item.name}">
								${title}
							</a>
						</span>
					</div>
					${details}
				</div>
			</div>
		`;
    }
    get_attached_images() {
      return frappe.call({
        method: "frappe.core.api.file.get_attached_images",
        args: {
          doctype: this.doctype,
          names: this.items.map((i2) => i2.name)
        }
      }).then((r) => {
        this.images_map = Object.assign(this.images_map || {}, r.message);
      });
    }
    setup_gallery() {
      var me2 = this;
      this.gallery = new frappe.views.GalleryView({
        doctype: this.doctype,
        items: this.items,
        wrapper: this.$result,
        images_map: this.images_map
      });
      this.$result.on("click", ".zoom-view", function(e) {
        e.preventDefault();
        e.stopPropagation();
        var name = $(this).data().name;
        name = decodeURIComponent(name);
        me2.gallery.show(name);
        return false;
      });
    }
    get required_libs() {
      return [
        "assets/frappe/node_modules/photoswipe/src/photoswipe.css",
        "photoswipe.bundle.js"
      ];
    }
  };
  frappe.views.GalleryView = class GalleryView {
    constructor(opts) {
      $.extend(this, opts);
      var me2 = this;
      me2.prepare();
    }
    prepare() {
      this.pswp_root = $("body > .pswp");
      if (this.pswp_root.length === 0) {
        var pswp = frappe.render_template("photoswipe_dom");
        this.pswp_root = $(pswp).appendTo("body");
      }
    }
    prepare_pswp_items(_items, _images_map) {
      var me2 = this;
      if (_items) {
        this.items = this.items.concat(_items);
        this.images_map = _images_map;
      }
      return new Promise((resolve) => {
        const items = this.items.filter((i2) => i2.image !== null).map(function(i2) {
          const query = 'img[data-name="' + i2._name + '"]';
          let el = me2.wrapper.find(query).get(0);
          let width, height;
          if (el) {
            width = el.naturalWidth;
            height = el.naturalHeight;
          }
          if (!el) {
            el = me2.wrapper.find('.image-field[data-name="' + i2._name + '"]').get(0);
            width = el.getBoundingClientRect().width;
            height = el.getBoundingClientRect().height;
          }
          return {
            src: i2._image_url,
            name: i2.name,
            width,
            height
          };
        });
        this.pswp_items = items;
        resolve();
      });
    }
    show(docname) {
      this.prepare_pswp_items().then(() => this._show(docname));
    }
    _show(docname) {
      const items = this.pswp_items;
      const item_index = items.findIndex((item) => item.name === docname);
      var options = {
        index: item_index,
        history: false,
        shareEl: false,
        dataSource: items
      };
      this.pswp = new frappe.PhotoSwipe(options);
      this.pswp.init();
    }
  };

  // frappe/public/js/frappe/views/map/map_view.js
  frappe.provide("frappe.utils");
  frappe.provide("frappe.views");
  frappe.views.MapView = class MapView extends frappe.views.ListView {
    get view_name() {
      return "Map";
    }
    setup_defaults() {
      super.setup_defaults();
      this.page_title = __("{0} Map", [this.page_title]);
      this.setup_map_type();
    }
    setup_map_type() {
      if (this.meta.fields.find(
        (i2) => i2.fieldname === "location" && i2.fieldtype === "Geolocation"
      )) {
        this.type = "location_field";
        this._add_field("location");
      } else if (this.meta.fields.find((i2) => i2.fieldname === "latitude") && this.meta.fields.find((i2) => i2.fieldname === "longitude")) {
        this.type = "coordinates";
        this._add_field("latitude");
        this._add_field("longitude");
      }
    }
    setup_view() {
      this.map_id = frappe.dom.get_unique_id();
      this.$result.html(`<div id="${this.map_id}" class="map-view-container"></div>`);
      L.Icon.Default.imagePath = frappe.utils.map_defaults.image_path;
      this.map = L.map(this.map_id).setView(
        frappe.utils.map_defaults.center,
        frappe.utils.map_defaults.zoom
      );
      this.streetLayer = L.tileLayer(
        frappe.utils.map_defaults.tiles.default_tile.url,
        frappe.utils.map_defaults.tiles.default_tile.options
      );
      this.satelliteLayer = L.tileLayer(
        frappe.utils.map_defaults.tiles.satellite_tile.url,
        frappe.utils.map_defaults.tiles.satellite_tile.options
      );
      this.labelsLayer = L.tileLayer(
        frappe.utils.map_defaults.tiles.labels_tail.url,
        frappe.utils.map_defaults.tiles.labels_tail.options
      );
      this.terrainLayer = L.tileLayer(
        frappe.utils.map_defaults.tiles.terrain_lines_tail.url,
        frappe.utils.map_defaults.tiles.terrain_lines_tail.options
      );
      this.streetLayer.addTo(this.map);
      this.bind_leaflet_layers_control();
      this.bind_leaflet_locate_control();
      L.control.scale().addTo(this.map);
      if (!this.bound_event_listeners) {
        this.bind_leaflet_event_listeners();
      }
    }
    render() {
      const coords = this.convert_to_geojson(this.data);
      this.render_map_data(coords);
      this.$paging_area.find(".level-left").append("<div></div>");
    }
    convert_to_geojson(data) {
      return this.type === "location_field" ? this.get_location_data(data) : this.get_coordinates_data(data);
    }
    get_coordinates_data(data) {
      return data.map((row) => this.create_gps_marker(row)).filter(Boolean);
    }
    get_location_data(data) {
      return data.reduce((acc, row) => {
        const location2 = this.parse_location_field(row);
        if (location2) {
          acc.push(...location2);
        }
        return acc;
      }, []);
    }
    get_feature_properties(row) {
      return {
        name: row.name
      };
    }
    parse_location_field(row) {
      const location2 = JSON.parse(row["location"]);
      if (!location2) {
        return;
      }
      for (const feature of location2["features"]) {
        feature["properties"] = __spreadValues(__spreadValues({}, feature["properties"] || {}), this.get_feature_properties(row));
      }
      return location2["features"];
    }
    create_gps_marker(row) {
      if (!row.latitude || !row.longitude) {
        return;
      }
      return {
        type: "Feature",
        properties: this.get_feature_properties(row),
        geometry: {
          type: "Point",
          coordinates: [parseFloat(row.longitude), parseFloat(row.latitude)]
        }
      };
    }
    get_popup_content(feature) {
      return frappe.utils.get_form_link(this.doctype, feature.properties.name, true);
    }
    render_map_data(features) {
      if (this.markerLayer) {
        this.map.removeLayer(this.markerLayer);
      }
      if (features && features.length) {
        this.markerLayer = L.featureGroup();
        features.forEach((feature) => {
          const marker = L.geoJSON(feature).bindPopup(this.get_popup_content(feature));
          this.markerLayer.addLayer(marker);
        });
        this.markerLayer.addTo(this.map);
        this.map.fitBounds(this.markerLayer.getBounds());
      }
    }
    bind_leaflet_layers_control() {
      const baseLayers = {
        Default: this.streetLayer,
        Satellite: this.satelliteLayer
      };
      const overlays = {
        Labels: this.labelsLayer,
        Terrain: this.terrainLayer
      };
      L.control.layers(baseLayers, overlays).addTo(this.map);
      this.display_leaflet_overlays_control("none");
    }
    bind_leaflet_locate_control() {
      this.locate_control = L.control.locate({ position: "topright" });
      this.locate_control.addTo(this.map);
    }
    display_leaflet_overlays_control(display = "") {
      const layerControlContainer = document.querySelector(".leaflet-control-layers-overlays");
      const separator = document.querySelector(".leaflet-control-layers-separator");
      if (layerControlContainer) {
        layerControlContainer.style.display = display;
      }
      if (separator) {
        separator.style.display = display;
      }
    }
    bind_leaflet_event_listeners() {
      this.bound_event_listeners = true;
      this.map.on("baselayerchange", (e) => {
        if (e.name === "Satellite") {
          this.display_leaflet_overlays_control();
        } else {
          this.display_leaflet_overlays_control("none");
          Object.values(this.map._layers).forEach((layer) => {
            if (layer instanceof L.TileLayer && (layer._url === frappe.utils.map_defaults.tiles.labels_tail.url || layer._url === frappe.utils.map_defaults.tiles.terrain_lines_tail.url)) {
              this.map.removeLayer(layer);
            }
          });
        }
      });
    }
  };

  // frappe/public/js/frappe/views/kanban/kanban_settings.js
  var KanbanSettings = class {
    constructor({ kanbanview, doctype, meta, settings }) {
      if (!doctype) {
        frappe.throw(__("DocType required"));
      }
      this.kanbanview = kanbanview;
      this.doctype = doctype;
      this.meta = meta;
      this.settings = settings;
      this.dialog = null;
      this.fields = this.settings && this.settings.fields;
      frappe.model.with_doctype("List View Settings", () => {
        this.make();
        this.get_fields();
        this.setup_fields();
        this.setup_remove_fields();
        this.add_new_fields();
        this.show_dialog();
      });
    }
    make() {
      this.dialog = new frappe.ui.Dialog({
        title: __("{0} Settings", [__(this.doctype)]),
        fields: [
          {
            fieldname: "show_labels",
            label: __("Show Labels"),
            fieldtype: "Check"
          },
          {
            fieldname: "fields_html",
            fieldtype: "HTML"
          },
          {
            fieldname: "fields",
            fieldtype: "Code",
            hidden: 1
          }
        ]
      });
      this.dialog.set_values(this.settings);
      this.dialog.set_primary_action(__("Save"), () => {
        frappe.show_alert({
          message: __("Saving"),
          indicator: "green"
        });
        frappe.call({
          method: "frappe.desk.doctype.kanban_board.kanban_board.save_settings",
          args: {
            board_name: this.settings.name,
            settings: this.dialog.get_values()
          },
          callback: (r) => {
            this.kanbanview.board = r.message;
            this.kanbanview.render();
            this.dialog.hide();
          }
        });
      });
    }
    refresh() {
      this.setup_fields();
      this.add_new_fields();
      this.setup_remove_fields();
    }
    show_dialog() {
      if (!this.settings.fields) {
        this.update_fields();
      }
      this.dialog.show();
    }
    setup_fields() {
      const fields_html = this.dialog.get_field("fields_html");
      const wrapper = fields_html.$wrapper[0];
      let fields = "";
      for (let fieldname of this.fields) {
        let field = this.get_docfield(fieldname);
        fields += `
				<div class="control-input flex align-center form-control fields_order sortable"
					style="display: block; margin-bottom: 5px;"
					data-fieldname="${field.fieldname}"
					data-label="${field.label}"
					data-type="${field.type}">

					<div class="row">
						<div class="col-md-1">
							${frappe.utils.icon("drag", "xs", "", "", "sortable-handle")}
						</div>
						<div class="col-md-10" style="padding-left:0px;">
							${__(field.label, null, field.parent)}
						</div>
						<div class="col-md-1">
							<a class="text-muted remove-field" data-fieldname="${field.fieldname}">
								${frappe.utils.icon("delete", "xs")}
							</a>
						</div>
					</div>
				</div>`;
      }
      fields_html.html(`
			<div class="form-group">
				<div class="clearfix">
					<label class="control-label" style="padding-right: 0px;">${__("Fields")}</label>
				</div>
				<div class="control-input-wrapper">
				${fields}
				</div>
				<p class="help-box small text-muted">
					<a class="add-new-fields text-muted">
						${__("+ Add / Remove Fields")}
					</a>
				</p>
			</div>
		`);
      new Sortable(wrapper.getElementsByClassName("control-input-wrapper")[0], {
        handle: ".sortable-handle",
        draggable: ".sortable",
        onUpdate: (params) => {
          this.fields.splice(params.newIndex, 0, this.fields.splice(params.oldIndex, 1)[0]);
          this.dialog.set_value("fields", JSON.stringify(this.fields));
          this.refresh();
        }
      });
    }
    add_new_fields() {
      let add_new_fields = this.get_dialog_fields_wrapper().getElementsByClassName("add-new-fields")[0];
      add_new_fields.onclick = () => this.show_column_selector();
    }
    setup_remove_fields() {
      let remove_fields = this.get_dialog_fields_wrapper().getElementsByClassName("remove-field");
      for (let idx = 0; idx < remove_fields.length; idx++) {
        remove_fields.item(idx).onclick = () => this.remove_fields(remove_fields.item(idx).getAttribute("data-fieldname"));
      }
    }
    get_dialog_fields_wrapper() {
      return this.dialog.get_field("fields_html").$wrapper[0];
    }
    remove_fields(fieldname) {
      this.fields = this.fields.filter((field) => field !== fieldname);
      this.dialog.set_value("fields", JSON.stringify(this.fields));
      this.refresh();
    }
    update_fields() {
      const wrapper = this.dialog.get_field("fields_html").$wrapper[0];
      let fields_order = wrapper.getElementsByClassName("fields_order");
      this.fields = [];
      for (let idx = 0; idx < fields_order.length; idx++) {
        this.fields.push(fields_order.item(idx).getAttribute("data-fieldname"));
      }
      this.dialog.set_value("fields", JSON.stringify(this.fields));
    }
    show_column_selector() {
      let dialog = new frappe.ui.Dialog({
        title: __("{0} Fields", [__(this.doctype)]),
        fields: [
          {
            label: __("Select Fields"),
            fieldtype: "MultiCheck",
            fieldname: "fields",
            options: this.get_multiselect_fields(),
            columns: 2
          }
        ]
      });
      dialog.set_primary_action(__("Save"), () => {
        this.fields = dialog.get_values().fields || [];
        this.dialog.set_value("fields", JSON.stringify(this.fields));
        this.refresh();
        dialog.hide();
      });
      dialog.show();
    }
    get_fields() {
      this.fields = this.settings.fields;
      this.fields.uniqBy((f) => f.fieldname);
    }
    get_docfield(field_name) {
      return frappe.meta.get_docfield(this.doctype, field_name) || frappe.model.get_std_field(field_name);
    }
    get_multiselect_fields() {
      const ignore_fields = [
        "idx",
        "lft",
        "rgt",
        "old_parent",
        "_user_tags",
        "_liked_by",
        "_comments",
        "_assign",
        this.meta.title_field || "name"
      ];
      const ignore_fieldtypes = [
        "Attach Image",
        "Text Editor",
        "HTML Editor",
        "Code",
        "Color",
        ...frappe.model.no_value_type
      ];
      return frappe.model.std_fields.concat(this.kanbanview.get_fields_in_list_view()).filter(
        (field) => !ignore_fields.includes(field.fieldname) && !ignore_fieldtypes.includes(field.fieldtype)
      ).map((field) => {
        return {
          label: __(field.label, null, field.parent),
          value: field.fieldname,
          checked: this.fields.includes(field.fieldname)
        };
      });
    }
  };

  // frappe/public/js/frappe/views/kanban/kanban_view.js
  frappe.provide("frappe.views");
  var _a2;
  frappe.views.KanbanView = (_a2 = class extends frappe.views.ListView {
    static load_last_view() {
      const route = frappe.get_route();
      if (route.length === 3) {
        const doctype = route[1];
        const user_settings = frappe.get_user_settings(doctype)["Kanban"] || {};
        if (!user_settings.last_kanban_board) {
          return new frappe.views.KanbanView({ doctype });
        }
        route.push(user_settings.last_kanban_board);
        frappe.set_route(route);
        return true;
      }
      return false;
    }
    get view_name() {
      return "Kanban";
    }
    show() {
      frappe.views.KanbanView.get_kanbans(this.doctype).then((kanbans) => {
        var _a3;
        frappe.route_options = {};
        if (!kanbans.length) {
          return frappe.views.KanbanView.show_kanban_dialog(this.doctype, true);
        } else if (kanbans.length && frappe.get_route().length !== 4) {
          const last_board = (_a3 = frappe.get_user_settings(this.doctype)["Kanban"]) == null ? void 0 : _a3.last_kanban_board;
          if (last_board && kanbans.includes(last_board)) {
            frappe.set_route("List", this.doctype, "Kanban", last_board);
            return;
          } else {
            const first_board = kanbans[0];
            frappe.set_route("List", this.doctype, "Kanban", first_board.name);
            return;
          }
        } else {
          this.kanbans = kanbans;
          return frappe.run_serially([
            () => this.show_skeleton(),
            () => this.fetch_meta(),
            () => this.hide_skeleton(),
            () => this.check_permissions(),
            () => this.init(),
            () => this.before_refresh(),
            () => this.refresh()
          ]);
        }
      });
    }
    init() {
      return super.init().then(() => {
        let menu_length = this.page.menu.find(".dropdown-item").length;
        if (menu_length === 1) {
          this.page.hide_menu();
        }
      });
    }
    setup_defaults() {
      return super.setup_defaults().then(() => {
        let get_board_name = () => {
          return this.kanbans.length && this.kanbans[0].name;
        };
        this.board_name = frappe.get_route()[3] || get_board_name() || null;
        this.page_title = __(this.board_name);
        this.card_meta = this.get_card_meta();
        this.page_length = 0;
        return frappe.run_serially([
          () => this.set_board_perms_and_push_menu_items(),
          () => this.get_board()
        ]);
      });
    }
    set_board_perms_and_push_menu_items() {
      return frappe.call({
        method: "frappe.client.get_doc_permissions",
        args: {
          doctype: "Kanban Board",
          docname: this.board_name
        },
        callback: (result) => {
          this.board_perms = result.message.permissions || {};
          this.push_menu_items();
        }
      });
    }
    push_menu_items() {
      if (this.board_perms.write) {
        this.menu_items.push({
          label: __("Save filters"),
          action: () => {
            this.save_kanban_board_filters();
          }
        });
      }
      if (this.board_perms.delete) {
        this.menu_items.push({
          label: __("Delete Kanban Board"),
          action: () => {
            frappe.confirm(__("Are you sure you want to proceed?"), () => {
              frappe.db.delete_doc("Kanban Board", this.board_name).then(() => {
                frappe.show_alert(`Kanban Board ${this.board_name} deleted.`);
                frappe.set_route("List", this.doctype, "List");
              });
            });
          }
        });
      }
    }
    setup_paging_area() {
    }
    set_result_height() {
    }
    toggle_result_area() {
      this.$result.toggle(this.data.length > 0);
    }
    get_board() {
      return frappe.db.get_doc("Kanban Board", this.board_name).then((board) => {
        this.board = board;
        this.board.filters_array = JSON.parse(this.board.filters || "[]");
        this.board.fields = JSON.parse(this.board.fields || "[]");
        this.filters = this.board.filters_array;
      });
    }
    setup_page() {
      this.hide_page_form = true;
      this.hide_card_layout = true;
      this.hide_sort_selector = true;
      super.setup_page();
    }
    setup_view() {
      if (this.board.columns.filter((col) => col.status !== "Archived").length > 4) {
        this.page.container.addClass("full-width");
      }
      this.setup_realtime_updates();
      this.setup_like();
    }
    set_fields() {
      this.fields = [];
      this._add_field("name");
      this._add_field("creation");
      this._add_field(this.board.field_name, this.board.reference_doctype);
      this._add_field(this.card_meta.title_field);
      this._add_field("_assign");
      this._add_field("_user_tags");
      this._add_field("_liked_by");
      this._add_field("_comments");
      this._add_field("owner");
      if (this.board.fields && Array.isArray(this.board.fields)) {
        this.board.fields.forEach((field_spec) => {
          const fieldname = typeof field_spec === "string" ? field_spec : field_spec == null ? void 0 : field_spec.fieldname;
          if (fieldname)
            this._add_field(fieldname);
        });
      }
      if (this.meta.image_field)
        this._add_field(this.meta.image_field);
      if (frappe.meta.has_field(this.doctype, "color"))
        this._add_field("color");
    }
    before_render() {
      frappe.model.user_settings.save(this.doctype, "last_view", this.view_name);
      this.save_view_user_settings({
        last_kanban_board: this.board_name
      });
    }
    render_list() {
    }
    on_filter_change() {
      if (!this.board_perms.write)
        return;
      if (JSON.stringify(this.board.filters_array) !== JSON.stringify(this.filter_area.get())) {
        this.page.set_indicator(__("Not Saved"), "orange");
      } else {
        this.page.clear_indicator();
      }
    }
    save_kanban_board_filters() {
      const filters = this.filter_area.get();
      frappe.db.set_value("Kanban Board", this.board_name, "filters", filters).then((r) => {
        if (r.exc) {
          frappe.show_alert({
            indicator: "red",
            message: __("There was an error saving filters")
          });
          return;
        }
        frappe.show_alert({
          indicator: "green",
          message: __("Filters saved")
        });
        this.board.filters_array = filters;
        this.on_filter_change();
      });
    }
    get_fields() {
      return super.get_fields();
    }
    render() {
      const board_name = this.board_name;
      if (!this.kanban) {
        this.kanban = new frappe.views.KanbanBoard({
          doctype: this.doctype,
          board: this.board,
          board_name,
          cards: this.data,
          card_meta: this.card_meta,
          wrapper: this.$result,
          cur_list: this,
          user_settings: this.view_user_settings
        });
      } else if (board_name === this.kanban.board_name) {
        this.$result.empty();
        this.kanban.update(this.data);
      }
    }
    get_card_meta() {
      var meta = frappe.get_meta(this.doctype);
      let route_options = __spreadValues({}, frappe.route_options);
      var doc = frappe.model.get_new_doc(this.doctype);
      frappe.route_options = route_options;
      var title_field = null;
      var quick_entry = false;
      if (this.meta.title_field) {
        title_field = frappe.meta.get_field(this.doctype, this.meta.title_field);
      }
      this.meta.fields.forEach((df) => {
        const is_valid_field = ["Data", "Text", "Small Text", "Text Editor"].includes(df.fieldtype) && !df.hidden;
        if (is_valid_field && !title_field) {
          title_field = df;
        }
      });
      var mandatory = meta.fields.filter((df) => df.reqd && !doc[df.fieldname]);
      if (mandatory.some((df) => frappe.model.table_fields.includes(df.fieldtype)) || mandatory.length > 1) {
        quick_entry = true;
      }
      if (!title_field) {
        title_field = frappe.meta.get_field(this.doctype, "name");
      }
      return {
        quick_entry,
        title_field
      };
    }
    get_view_settings() {
      return {
        label: __("Kanban Settings", null, "Button in kanban view menu"),
        action: () => this.show_kanban_settings(),
        standard: true
      };
    }
    show_kanban_settings() {
      frappe.model.with_doctype(this.doctype, () => {
        new KanbanSettings({
          kanbanview: this,
          doctype: this.doctype,
          settings: this.board,
          meta: frappe.get_meta(this.doctype)
        });
      });
    }
    get required_libs() {
      return "kanban_board.bundle.js";
    }
  }, __publicField(_a2, "full_page", true), __publicField(_a2, "no_sidebar", true), _a2);
  frappe.views.KanbanView.get_kanbans = function(doctype) {
    let kanbans = [];
    return get_kanban_boards().then((kanban_boards) => {
      if (kanban_boards) {
        kanban_boards.forEach((board) => {
          let route = `/desk/${frappe.router.slug(board.reference_doctype)}/view/kanban/${board.name}`;
          kanbans.push({ name: board.name, route });
        });
      }
      return kanbans;
    });
    function get_kanban_boards() {
      return frappe.call("frappe.desk.doctype.kanban_board.kanban_board.get_kanban_boards", { doctype }).then((r) => r.message);
    }
  };
  frappe.views.KanbanView.show_kanban_dialog = function(doctype) {
    let dialog = new_kanban_dialog();
    dialog.show();
    function make_kanban_board(board_name, field_name, project) {
      return frappe.call({
        method: "frappe.desk.doctype.kanban_board.kanban_board.quick_kanban_board",
        args: {
          doctype,
          board_name,
          field_name,
          project
        },
        callback: function(r) {
          var kb = r.message;
          if (kb.filters) {
            frappe.provide("frappe.kanban_filters");
            frappe.kanban_filters[kb.kanban_board_name] = kb.filters;
          }
          frappe.set_route("List", doctype, "Kanban", kb.kanban_board_name);
        }
      });
    }
    function new_kanban_dialog() {
      const select_fields = frappe.get_meta(doctype).fields.filter((df) => {
        return df.fieldtype === "Select" && df.fieldname !== "kanban_column";
      });
      const dialog_fields = get_fields_for_dialog(select_fields);
      const to_save = select_fields.length > 0;
      const primary_action_label = to_save ? __("Save") : __("Customize Form");
      const dialog_title = to_save ? __("New Kanban Board") : __("No Select Field Found");
      let primary_action = () => {
        if (to_save) {
          const values = dialog.get_values();
          make_kanban_board(values.board_name, values.field_name, values.project).then(
            () => dialog.hide(),
            (err) => frappe.msgprint(err)
          );
        } else {
          frappe.set_route("Form", "Customize Form", { doc_type: doctype });
        }
      };
      return new frappe.ui.Dialog({
        title: dialog_title,
        fields: dialog_fields,
        primary_action_label,
        primary_action
      });
    }
    function get_fields_for_dialog(select_fields) {
      if (!select_fields.length) {
        return [
          {
            fieldtype: "HTML",
            options: `
					<div>
						<p class="text-medium">
						${__(
              'No fields found that can be used as a Kanban Column. Use the Customize Form to add a Custom Field of type "Select".'
            )}
						</p>
					</div>
				`
          }
        ];
      }
      let fields = [
        {
          fieldtype: "Data",
          fieldname: "board_name",
          label: __("Kanban Board Name"),
          reqd: 1,
          description: ["Note", "ToDo"].includes(doctype) ? __("This Kanban Board will be private") : ""
        },
        {
          fieldtype: "Select",
          fieldname: "field_name",
          label: __("Columns based on"),
          options: select_fields.map((df) => ({ label: df.label, value: df.fieldname })),
          default: select_fields[0],
          reqd: 1
        }
      ];
      if (doctype === "Task") {
        fields.push({
          fieldtype: "Link",
          fieldname: "project",
          label: __("Project"),
          options: "Project"
        });
      }
      return fields;
    }
  };

  // frappe/public/js/frappe/views/inbox/inbox_view.js
  frappe.provide("frappe.views");
  frappe.views.InboxView = class InboxView extends frappe.views.ListView {
    static load_last_view() {
      const route = frappe.get_route();
      if (!route[3] && frappe.boot.email_accounts.length) {
        let email_account;
        if (frappe.boot.email_accounts[0].email_id == "All Accounts") {
          email_account = "All Accounts";
        } else {
          email_account = frappe.boot.email_accounts[0].email_account;
        }
        frappe.set_route("List", "Communication", "Inbox", email_account);
        return true;
      } else if (!route[3] || route[3] !== "All Accounts" && !is_valid(route[3])) {
        frappe.throw(
          __(
            "No email account associated with the User. Please add an account under User > Email Inbox."
          )
        );
      }
      return false;
      function is_valid(email_account) {
        return frappe.boot.email_accounts.find((d) => d.email_account === email_account);
      }
    }
    get view_name() {
      return "Inbox";
    }
    show() {
      super.show();
      this.save_view_user_settings({
        last_email_account: this.current_email_account
      });
    }
    setup_defaults() {
      super.setup_defaults();
      this.sort_by = this.view_user_settings.sort_by || "communication_date";
      this.sort_order = this.view_user_settings.sort_order || "desc";
      this.email_account = frappe.get_route()[3];
      this.page_title = this.email_account;
      this.filters = this.get_inbox_filters();
    }
    setup_columns() {
      this.columns = [];
      this.columns.push({
        type: "Subject",
        df: {
          label: __("Subject"),
          fieldname: "subject"
        }
      });
      this.columns.push({
        type: "Field",
        df: {
          label: this.is_sent_emails ? __("To") : __("From"),
          fieldname: this.is_sent_emails ? "recipients" : "sender"
        }
      });
    }
    get_seen_class(doc) {
      return Boolean(doc.seen) || JSON.parse(doc._seen || "[]").includes(frappe.session.user) ? "" : "bold";
    }
    get is_sent_emails() {
      const f = this.filter_area.get().find((filter) => filter[1] === "sent_or_received");
      return f && f[3] === "Sent";
    }
    render_header() {
      this.$result.find(".list-row-head").remove();
      this.$result.prepend(this.get_header_html());
    }
    render() {
      this.setup_columns();
      this.render_header();
      this.render_list();
      this.on_row_checked();
      this.render_count();
    }
    get_meta_html(email) {
      const attachment = email.has_attachment ? `<span class="fa fa-paperclip fa-large" title="${__("Has Attachments")}"></span>` : "";
      let link = "";
      if (email.reference_doctype && email.reference_doctype !== this.doctype) {
        link = `<a class="text-muted grey"
				href="${frappe.utils.get_form_link(email.reference_doctype, email.reference_name)}"
				title="${__("Linked with {0}", [email.reference_doctype])}">
				<i class="fa fa-link fa-large"></i>
			</a>`;
      }
      const communication_date = comment_when(email.communication_date, true);
      const status = email.status == "Closed" ? `<span class="fa fa-check fa-large" title="${__(email.status)}"></span>` : email.status == "Replied" ? `<span class="fa fa-mail-reply fa-large" title="${__(email.status)}"></span>` : "";
      return `
			<div class="level-item list-row-activity">
				${link}
				${attachment}
				${status}
				${communication_date}
			</div>
		`;
    }
    get_inbox_filters() {
      var email_account = this.email_account;
      var default_filters = [
        ["Communication", "communication_type", "=", "Communication", true],
        ["Communication", "communication_medium", "=", "Email", true]
      ];
      var filters = [];
      if (email_account === "Sent") {
        filters = default_filters.concat([
          ["Communication", "sent_or_received", "=", "Sent", true],
          ["Communication", "email_status", "not in", "Spam,Trash", true]
        ]);
      } else if (["Spam", "Trash"].includes(email_account)) {
        filters = default_filters.concat([
          ["Communication", "email_status", "=", email_account, true],
          ["Communication", "email_account", "in", frappe.boot.all_accounts, true]
        ]);
      } else {
        var op = "=";
        if (email_account == "All Accounts") {
          op = "in";
          email_account = frappe.boot.all_accounts;
        }
        filters = default_filters.concat([
          ["Communication", "sent_or_received", "=", "Received", true],
          ["Communication", "status", "=", "Open", true],
          ["Communication", "email_account", op, email_account, true],
          ["Communication", "email_status", "not in", "Spam,Trash", true]
        ]);
      }
      return filters;
    }
    get_no_result_message() {
      var email_account = this.email_account;
      var args;
      if (["Spam", "Trash"].includes(email_account)) {
        return __("No {0} mail", [email_account]);
      } else if (!email_account && !frappe.boot.email_accounts.length) {
        args = {
          doctype: "Email Account",
          msg: __("No Email Account"),
          label: __("New Email Account")
        };
      } else {
        args = {
          doctype: "Communication",
          msg: __("No Emails"),
          label: __("Compose Email")
        };
      }
      const html = frappe.model.can_create(args.doctype) ? `<p>${args.msg}</p>
			<p>
				<button class="btn btn-primary btn-sm btn-new-doc">
					${args.label}
				</button>
			</p>
			` : `<p>${__("No Email Accounts Assigned")}</p>`;
      return `
			<div class="msg-box no-border">
				${html}
			</div>
		`;
    }
    make_new_doc() {
      if (!this.email_account && !frappe.boot.email_accounts.length) {
        frappe.route_options = {
          email_id: frappe.session.user_email
        };
        frappe.new_doc("Email Account");
      } else {
        new frappe.views.CommunicationComposer();
      }
    }
  };

  // frappe/public/js/frappe/views/file/file_view.js
  frappe.provide("frappe.views");
  frappe.views.FileView = class FileView extends frappe.views.ListView {
    static load_last_view() {
      const route = frappe.get_route();
      if (route.length === 2) {
        const view_user_settings = frappe.get_user_settings("File", "File");
        frappe.set_route(
          "List",
          "File",
          view_user_settings.last_folder || frappe.boot.home_folder
        );
        return true;
      }
      return redirect_to_home_if_invalid_route();
    }
    get view_name() {
      return "File";
    }
    show() {
      if (!redirect_to_home_if_invalid_route()) {
        super.show();
      }
    }
    setup_view() {
      this.render_header();
      this.setup_events();
      this.$page.find(".layout-main-section-wrapper").addClass("file-view");
      this.add_file_action_buttons();
      this.page.add_button(__("Toggle Grid View"), () => {
        frappe.views.FileView.grid_view = !frappe.views.FileView.grid_view;
        this.refresh();
      });
    }
    setup_no_result_area() {
      this.$no_result = $(`<div class="no-result">
			<div class="breadcrumbs">${this.get_breadcrumbs_html()}</div>
			<div class="text-muted flex justify-center align-center">
				${this.get_no_result_message()}
			</div>
		</div>`).hide();
      this.$frappe_list.append(this.$no_result);
    }
    get_args() {
      let args = super.get_args();
      if (frappe.views.FileView.grid_view) {
        Object.assign(args, {
          order_by: `is_folder desc, ${this.sort_by} ${this.sort_order}`
        });
      }
      return args;
    }
    set_breadcrumbs() {
      const route = frappe.get_route();
      route.splice(-1);
      const last_folder = route[route.length - 1];
      if (last_folder === "File")
        return;
      frappe.breadcrumbs.add({
        type: "Custom",
        label: __("Home"),
        route: "/desk/List/File/Home"
      });
    }
    setup_defaults() {
      return super.setup_defaults().then(() => {
        this.page_title = __("File Manager");
        const route = frappe.get_route();
        this.current_folder = route.slice(2).join("/") || "Home";
        this.filters = [["File", "folder", "=", this.current_folder, true]];
        this.order_by = this.view_user_settings.order_by || "file_name asc";
        this.menu_items = this.menu_items.concat(this.file_menu_items());
      });
    }
    make_new_doc() {
      new frappe.ui.FileUploader({
        folder: this.current_folder
      });
    }
    file_menu_items() {
      return [
        {
          label: __("Home"),
          action: () => {
            frappe.set_route("List", "File", "Home");
          }
        },
        {
          label: __("New Folder"),
          action: () => {
            frappe.prompt(
              __("Name"),
              (values) => {
                if (values.value.indexOf("/") > -1) {
                  frappe.throw(__("Folder name should not include '/' (slash)"));
                }
                const data = {
                  file_name: values.value,
                  folder: this.current_folder
                };
                frappe.call({
                  method: "frappe.core.api.file.create_new_folder",
                  args: data
                });
              },
              __("Enter folder name"),
              __("Create")
            );
          }
        },
        {
          label: __("Import Zip"),
          action: () => {
            new frappe.ui.FileUploader({
              folder: this.current_folder,
              restrictions: {
                allowed_file_types: [".zip"]
              },
              on_success: (file) => {
                frappe.show_alert(__("Unzipping files..."));
                frappe.call("frappe.core.api.file.unzip_file", {
                  name: file.name
                }).then((r) => {
                  if (r.message) {
                    frappe.show_alert(__("Unzipped {0} files", [r.message]));
                  }
                });
              }
            });
          }
        }
      ];
    }
    add_file_action_buttons() {
      this.$cut_button = this.page.add_button(__("Cut"), () => {
        frappe.file_manager.cut(this.get_checked_items(), this.current_folder);
        this.$checks.parents(".file-wrapper").addClass("cut");
      }).hide();
      this.$paste_btn = this.page.add_button(__("Paste"), () => frappe.file_manager.paste(this.current_folder)).hide();
      this.page.add_actions_menu_item(__("Export as zip"), () => {
        let docnames = this.get_checked_items(true);
        if (docnames.length) {
          open_url_post("/api/method/frappe.core.api.file.zip_files", {
            files: JSON.stringify(docnames)
          });
        }
      });
    }
    set_fields() {
      this.fields = this.meta.fields.filter((df) => frappe.model.is_value_type(df.fieldtype) && !df.hidden).map((df) => df.fieldname).concat(["name", "modified", "creation"]);
    }
    prepare_data(data) {
      super.prepare_data(data);
      this.prepare_file_data();
    }
    prepare_file_data() {
      this.data = this.data.map((d) => this.prepare_datum(d));
      const { sort_by } = this.sort_selector;
      if (sort_by === "file_name") {
        this.data.sort((a, b) => {
          if (a.is_folder && !b.is_folder) {
            return -1;
          }
          if (!a.is_folder && b.is_folder) {
            return 1;
          }
          return 0;
        });
      }
    }
    prepare_datum(d) {
      let icon_class = "";
      let type = "";
      let title;
      if (d.is_folder) {
        icon_class = "folder-normal";
        type = "folder";
      } else if (frappe.utils.is_image_file(d.file_name)) {
        icon_class = "image";
        type = "image";
      } else if (frappe.utils.is_video_file(d.file_name)) {
        icon_class = "file-play";
        type = "video";
      } else {
        icon_class = "file";
        type = "file";
      }
      if (type === "folder") {
        title = this.get_folder_title(d.file_name);
      } else {
        title = d.file_name || d.file_url;
      }
      title = frappe.utils.escape_html(title);
      title = title.slice(0, 60);
      d._title = title;
      d.icon_class = icon_class;
      d._type = type;
      d.subject_html = `
			${frappe.utils.icon(icon_class)}
			<span>${title}</span>
			${d.is_private ? '<i class="fa fa-lock fa-fw text-warning"></i>' : ""}
		`;
      return d;
    }
    get_folder_title(folder_name) {
      if (["Home", "Attachments"].includes(folder_name)) {
        return __(folder_name);
      } else {
        return folder_name;
      }
    }
    before_render() {
      super.before_render();
      frappe.model.user_settings.save("File", "grid_view", frappe.views.FileView.grid_view);
      this.save_view_user_settings({
        last_folder: this.current_folder
      });
    }
    render() {
      this.$result.empty().removeClass("file-grid-view");
      if (frappe.views.FileView.grid_view) {
        this.prepare_file_data();
        this.render_grid_view();
      } else {
        super.render();
        this.render_header();
        this.render_count();
      }
    }
    after_render() {
    }
    render_list() {
      if (frappe.views.FileView.grid_view) {
        this.prepare_file_data();
        this.render_grid_view();
      } else {
        super.render_list();
      }
    }
    remove_list_items(names) {
      if (frappe.views.FileView.grid_view) {
        for (let name of names) {
          this.$result.find(`.file-wrapper[data-name='${name.replace(/'/g, "\\'")}']`).remove();
        }
      } else {
        super.remove_list_items(names);
      }
    }
    render_grid_view() {
      const base_url = frappe.urllib.get_base_url();
      let html = this.data.map((d) => {
        const icon_class = d.icon_class + "-large";
        const align_file_body_class = d._type == "image" ? "align-flex-start" : "align-center";
        const file_url = frappe.utils.escape_html(d.file_url);
        const absolute_file_url = base_url + file_url;
        let file_body_html = d._type == "image" ? `<div class="file-image"><img class="w-100" src="${file_url}" alt="${d.file_name}"></div>` : frappe.utils.icon(icon_class, {
          width: "40px",
          height: "45px"
        });
        const name = escape(d.name);
        const copy_url_btn = `
					<div class="copy-file-url hidden-xs" title="${__(
          "Copy File URL"
        )}" data-file-url="${absolute_file_url}">
						<svg class="es-icon es-line icon-sm" aria-hidden="true">
							<use class="" href="#es-line-copy-light"></use>
						</svg>
					</div>
					`;
        const draggable = d.type == "Folder" ? false : true;
        return `
				<a href="${this.get_route_url(d)}"
					draggable="${draggable}" class="file-wrapper ellipsis" data-name="${name}">
					<div class="file-header level w-100">
						<input class="level-item list-row-checkbox hidden-xs" type="checkbox" data-name="${name}">
						${!d.is_folder ? copy_url_btn : ""}
					</div>
					<div class="file-body ${align_file_body_class}">
						${file_body_html}
					</div>
					<div class="file-footer">
						<div class="file-title ellipsis">${d._title}</div>
						<div class="file-creation">${this.get_creation_date(d)}</div>
					</div>
				</a>
			`;
      }).join("");
      this.$result.addClass("file-grid-view");
      this.$result.empty().html(
        `<div class="file-grid">
				${html}
			</div>`
      );
    }
    get_breadcrumbs_html() {
      const route = frappe.get_route();
      const folders = route.slice(2);
      return folders.map((folder, i2) => {
        const title = this.get_folder_title(folder);
        if (i2 === folders.length - 1) {
          return `<span>${title}</span>`;
        }
        const route2 = folders.reduce((acc, curr, j) => {
          if (j <= i2) {
            acc += "/" + curr;
          }
          return acc;
        }, "/desk/file/view");
        return `<a href="${route2}">${title}</a>`;
      }).join("&nbsp;/&nbsp;");
    }
    get_header_html() {
      const breadcrumbs_html = this.get_breadcrumbs_html();
      let header_selector_html = !frappe.views.FileView.grid_view ? `<input class="level-item list-check-all hidden-xs" type="checkbox" title="${__(
        "Select All"
      )}">` : "";
      let header_columns_html = !frappe.views.FileView.grid_view ? `<div class="list-row-col ellipsis hidden-xs">
					<span>${__("Size")}</span>
				</div>
				<div class="list-row-col ellipsis hidden-xs">
					<span>${__("Type")}</span>
				</div>
				<div class="list-row-col ellipsis hidden-xs">
					<span>${__("Created")}</span>
				</div>` : "";
      let subject_html = `
			<div class="list-row-col list-subject level">
				${header_selector_html}
				<span class="level-item">${breadcrumbs_html}</span>
			</div>
			${header_columns_html}
		`;
      return this.get_header_html_skeleton(subject_html, '<span class="list-count"></span>');
    }
    get_route_url(file) {
      return file.is_folder ? "/desk/List/File/" + file.name : this.get_form_link(file);
    }
    get_creation_date(file) {
      const [date] = file.creation.split(" ");
      let created_on;
      if (date === frappe.datetime.now_date()) {
        created_on = comment_when(file.creation);
      } else {
        created_on = frappe.datetime.str_to_user(date);
      }
      return created_on;
    }
    get_left_html(file) {
      file = this.prepare_datum(file);
      const file_size = file.file_size ? frappe.form.formatters.FileSize(file.file_size) : "";
      const route_url = this.get_route_url(file);
      return `
			<div class="list-row-col ellipsis list-subject level">
				<span class="level-item file-select">
					<input class="list-row-checkbox"
						type="checkbox" data-name="${file.name}">
				</span>
				<span class="level-item  ellipsis" title="${frappe.utils.escape_html(file.file_name)}">
					<a class="ellipsis" href="${route_url}" title="${frappe.utils.escape_html(file.file_name)}">
						${file.subject_html}
					</a>
				</span>
			</div>
			<div class="list-row-col ellipsis hidden-xs text-muted">
				<span>${file_size}</span>
			</div>
			<div class="list-row-col ellipsis hidden-xs text-muted">
				<span>${file.file_type || ""}</span>
			</div>
			<div class="list-row-col ellipsis hidden-xs text-muted">
				<span>${this.get_creation_date(file)}</span>
			</div>
		`;
    }
    get_right_html(file) {
      return `
			<div class="level-item list-row-activity">
				${comment_when(file.modified)}
			</div>
		`;
    }
    setup_events() {
      super.setup_events();
      this.setup_drag_events();
      this.setup_copy_event();
    }
    setup_drag_events() {
      this.$result.on("dragstart", ".files .file-wrapper", (e) => {
        e.stopPropagation();
        e.originalEvent.dataTransfer.setData("Text", $(e.currentTarget).attr("data-name"));
        e.target.style.opacity = "0.4";
        frappe.file_manager.cut(
          [{ name: $(e.currentTarget).attr("data-name") }],
          this.current_folder
        );
      });
      this.$result.on(
        "dragover",
        (e) => {
          e.preventDefault();
        },
        false
      );
      this.$result.on("dragend", ".files .file-wrapper", (e) => {
        e.preventDefault();
        e.stopPropagation();
        e.target.style.opacity = "1";
      });
      this.$result.on("drop", (e) => {
        e.stopPropagation();
        e.preventDefault();
        const $el = $(e.target).parents(".file-wrapper");
        let dataTransfer = e.originalEvent.dataTransfer;
        if (!dataTransfer)
          return;
        if (dataTransfer.files && dataTransfer.files.length > 0) {
          new frappe.ui.FileUploader({
            files: dataTransfer.files,
            folder: this.current_folder
          });
        } else if (dataTransfer.getData("Text")) {
          if ($el.parents(".folders").length !== 0) {
            const file_name = dataTransfer.getData("Text");
            const folder_name = decodeURIComponent($el.attr("data-name"));
            frappe.file_manager.paste(folder_name);
            frappe.show_alert(`File ${file_name} moved to ${folder_name}`);
          }
        }
      });
    }
    setup_copy_event() {
      this.$result.on("click", ".copy-file-url", (e) => {
        frappe.utils.copy_to_clipboard(e.currentTarget.getAttribute("data-file-url"));
        e.preventDefault();
        e.stopPropagation();
      });
    }
    toggle_result_area() {
      super.toggle_result_area();
      this.toggle_cut_paste_buttons();
    }
    on_row_checked() {
      super.on_row_checked();
      this.toggle_cut_paste_buttons();
    }
    toggle_cut_paste_buttons() {
      const hide_paste_btn = !frappe.file_manager.can_paste || frappe.file_manager.old_folder === this.current_folder;
      const hide_cut_btn = !(this.$checks && this.$checks.length > 0);
      this.$paste_btn.toggle(!hide_paste_btn);
      this.$cut_button.toggle(!hide_cut_btn);
    }
  };
  frappe.views.FileView.grid_view = frappe.get_user_settings("File").grid_view || false;
  function redirect_to_home_if_invalid_route() {
    const route = frappe.get_route();
    if (route[2] === "List") {
      frappe.set_route("List", "File", "Home");
      return true;
    }
    return false;
  }

  // frappe/public/js/list.bundle.js
  var import_treeview = __toESM(require_treeview());

  // frappe/public/js/frappe/views/interaction.js
  frappe.provide("frappe.views");
  frappe.provide("frappe.interaction_settings");
  frappe.views.InteractionComposer = class InteractionComposer {
    constructor(opts) {
      $.extend(this, opts);
      this.make();
    }
    make() {
      let me2 = this;
      me2.dialog = new frappe.ui.Dialog({
        title: me2.title || me2.subject || __("New Activity"),
        no_submit_on_enter: true,
        fields: me2.get_fields(),
        primary_action_label: __("Create"),
        primary_action: function() {
          me2.create_action();
        }
      });
      $(document).on("upload_complete", function(event2, attachment) {
        if (me2.dialog.display) {
          let wrapper = $(me2.dialog.fields_dict.select_attachments.wrapper);
          let checked_items = wrapper.find("[data-file-name]:checked").map(function() {
            return $(this).attr("data-file-name");
          });
          me2.render_attach();
          checked_items.push(attachment.name);
          $.each(checked_items, function(i2, filename) {
            wrapper.find('[data-file-name="' + filename + '"]').prop("checked", true);
          });
        }
      });
      me2.prepare();
      me2.dialog.show();
    }
    get_fields() {
      let me2 = this;
      let interaction_docs = Object.keys(get_doc_mappings());
      return [
        {
          label: __("Reference"),
          fieldtype: "Select",
          fieldname: "interaction_type",
          options: interaction_docs,
          reqd: 1,
          onchange: () => {
            let values = me2.get_values();
            me2.get_fields().forEach((field) => {
              if (field.fieldname != "interaction_type") {
                me2.dialog.set_df_property(field.fieldname, "reqd", 0);
                me2.dialog.set_df_property(field.fieldname, "hidden", 0);
              }
            });
            me2.set_reqd_hidden_fields(values);
            me2.get_event_categories();
          }
        },
        {
          label: __("Category"),
          fieldtype: "Select",
          fieldname: "category",
          options: "",
          hidden: 1
        },
        { label: __("Public"), fieldtype: "Check", fieldname: "public", default: "0" },
        { fieldtype: "Column Break" },
        { label: __("Date"), fieldtype: "Datetime", fieldname: "due_date" },
        {
          label: __("Assigned To"),
          fieldtype: "Link",
          fieldname: "assigned_to",
          options: "User"
        },
        { fieldtype: "Section Break" },
        { label: __("Summary"), fieldtype: "Data", fieldname: "summary" },
        { fieldtype: "Section Break" },
        { fieldtype: "Text Editor", fieldname: "description" },
        { fieldtype: "Section Break" },
        {
          label: __("Select Attachments"),
          fieldtype: "HTML",
          fieldname: "select_attachments"
        }
      ];
    }
    get_event_categories() {
      let me2 = this;
      frappe.model.with_doctype("Event", () => {
        let categories = frappe.meta.get_docfield("Event", "event_category").options.split("\n");
        me2.dialog.get_input("category").empty().add_options(categories);
      });
    }
    prepare() {
      this.setup_attach();
    }
    set_reqd_hidden_fields(values) {
      let me2 = this;
      if (values && "interaction_type" in values) {
        let doc_mapping = get_doc_mappings();
        doc_mapping[values.interaction_type]["reqd_fields"].forEach((value) => {
          me2.dialog.set_df_property(value, "reqd", 1);
        });
        doc_mapping[values.interaction_type]["hidden_fields"].forEach((value) => {
          me2.dialog.set_df_property(value, "hidden", 1);
        });
      }
    }
    setup_attach() {
      var fields = this.dialog.fields_dict;
      var attach = $(fields.select_attachments.wrapper);
      if (!this.attachments) {
        this.attachments = [];
      }
      let args = {
        folder: "Home/Attachments",
        on_success: (attachment) => this.attachments.push(attachment)
      };
      if (this.frm) {
        args = {
          doctype: this.frm.doctype,
          docname: this.frm.docname,
          folder: "Home/Attachments",
          on_success: (attachment) => {
            this.frm.attachments.attachment_uploaded(attachment);
            this.render_attach();
          }
        };
      }
      $(
        "<h6 class='text-muted add-attachment' style='margin-top: 12px; cursor:pointer;'>" + __("Select Attachments") + "</h6><div class='attach-list'></div>			<p class='add-more-attachments'>			<a class='text-muted small'><i class='octicon octicon-plus' style='font-size: 12px'></i> " + __("Add Attachment") + "</a></p>"
      ).appendTo(attach.empty());
      attach.find(".add-more-attachments a").on("click", () => new frappe.ui.FileUploader(args));
      this.render_attach();
    }
    render_attach() {
      let fields = this.dialog.fields_dict;
      let attach = $(fields.select_attachments.wrapper).find(".attach-list").empty();
      let files = [];
      if (this.attachments && this.attachments.length) {
        files = files.concat(this.attachments);
      }
      if (cur_frm) {
        files = files.concat(cur_frm.get_files());
      }
      if (files.length) {
        $.each(files, function(i2, f) {
          if (!f.file_name)
            return;
          f.file_url = frappe.urllib.get_full_url(f.file_url);
          $(
            repl(
              '<p class="checkbox"><label><span><input type="checkbox" data-file-name="%(name)s"></input></span><span class="small">%(file_name)s</span> <a href="%(file_url)s" target="_blank" class="text-muted small"><i class="fa fa-share" style="vertical-align: middle; margin-left: 3px;"></i></label></p>',
              f
            )
          ).appendTo(attach);
        });
      }
    }
    create_action() {
      let me2 = this;
      let btn = me2.dialog.get_primary_btn();
      let form_values = this.get_values();
      if (!form_values)
        return;
      let selected_attachments = $.map(
        $(me2.dialog.wrapper).find("[data-file-name]:checked"),
        function(element) {
          return $(element).attr("data-file-name");
        }
      );
      me2.create_interaction(btn, form_values, selected_attachments);
    }
    get_values() {
      let me2 = this;
      let values = this.dialog.get_values(true);
      if (values) {
        values["reference_doctype"] = me2.frm.doc.doctype;
        values["reference_document"] = me2.frm.doc.name;
      }
      return values;
    }
    create_interaction(btn, form_values, selected_attachments) {
      let me2 = this;
      me2.dialog.hide();
      let field_map = get_doc_mappings();
      let interaction_values = {};
      Object.keys(form_values).forEach((value) => {
        interaction_values[field_map[form_values.interaction_type]["field_map"][value]] = form_values[value];
      });
      if ("event_type" in interaction_values) {
        interaction_values["event_type"] = form_values.public == 1 ? "Public" : "Private";
      }
      if (interaction_values["doctype"] == "Event") {
        interaction_values["event_participants"] = [
          {
            reference_doctype: form_values.reference_doctype,
            reference_docname: form_values.reference_document
          }
        ];
      }
      if (!("owner" in interaction_values)) {
        interaction_values["owner"] = frappe.session.user;
      }
      if (!("assigned_by" in interaction_values) && interaction_values["doctype"] == "ToDo") {
        interaction_values["assigned_by"] = frappe.session.user;
      }
      return frappe.call({
        method: "frappe.client.insert",
        args: { doc: interaction_values },
        btn,
        callback: function(r) {
          if (!r.exc) {
            frappe.show_alert({
              message: __("{0} created successfully", [form_values.interaction_type]),
              indicator: "green"
            });
            if (form_values.interaction_type === "Event" && "assigned_to" in form_values) {
              me2.assign_document(r.message, form_values["assigned_to"]);
            }
            if (selected_attachments) {
              me2.add_attachments(r.message, selected_attachments);
            }
            if (cur_frm) {
              cur_frm.reload_doc();
            }
          } else {
            frappe.msgprint(
              __("There were errors while creating the document. Please try again.")
            );
          }
        }
      });
    }
    assign_document(doc, assignee) {
      frappe.call({
        method: "frappe.desk.form.assign_to.add",
        args: {
          doctype: doc.doctype,
          name: doc.name,
          assign_to: JSON.stringify([assignee])
        },
        callback: function(r) {
          if (!r.exc) {
            frappe.show_alert({
              message: __("The document has been assigned to {0}", [assignee]),
              indicator: "green"
            });
            return;
          } else {
            frappe.show_alert({
              message: __("The document could not be correctly assigned"),
              indicator: "orange"
            });
            return;
          }
        }
      });
    }
    add_attachments(doc, attachments) {
      frappe.call({
        method: "frappe.utils.file_manager.add_attachments",
        args: {
          doctype: doc.doctype,
          name: doc.name,
          attachments: JSON.stringify(attachments)
        },
        callback: function(r) {
          if (!r.exc) {
            return;
          } else {
            frappe.show_alert({
              message: __(
                "The attachments could not be correctly linked to the new document"
              ),
              indicator: "orange"
            });
            return;
          }
        }
      });
    }
  };
  function get_doc_mappings() {
    return {
      Event: {
        field_map: {
          interaction_type: "doctype",
          summary: "subject",
          description: "description",
          category: "event_category",
          due_date: "starts_on",
          public: "event_type"
        },
        reqd_fields: ["summary", "due_date"],
        hidden_fields: []
      },
      ToDo: {
        field_map: {
          interaction_type: "doctype",
          description: "description",
          due_date: "date",
          reference_doctype: "reference_type",
          reference_document: "reference_name",
          assigned_to: "allocated_to"
        },
        reqd_fields: ["description"],
        hidden_fields: ["public", "category"]
      }
    };
  }

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/views/image/image_view_item_row.html
  frappe.templates["image_view_item_row"] = `<div class="image-view-item has-checkbox ellipsis">
	<div class="image-view-header doclist-row">
		<div class="list-value">
		{{ subject }}
		</div>
	</div>
	<!-- Image -->
	<div class="image-view-body">
		<a  data-name="{{ data.name }}"
			title="{{ data.name }}"
			href="/desk/Form/{{ data.doctype }}/{{ data.name }}"
		>
			<div class="image-field"
				data-name="{{ data.name }}"
				style="
				{% if (!data._image_url) { %}
					background-color: {{ color }};
				{% } %}
				border: 0px;"
			>
				{% if (!data._image_url) { %}
				<span class="placeholder-text">
					{%= frappe.get_abbr(data._title) %}
				</span>
				{% } %}
				{% if (data._image_url) { %}
				<img data-name="{{ data.name }}" src="{{ data._image_url }}" alt="{{data.title}}">
				{% } %}
				<button class="btn btn-default zoom-view" data-name="{{data.name}}">
					<i class="fa fa-search-plus"></i>
				</button>
			</div>
		</a>
	</div>
</div>
`;

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/views/image/photoswipe_dom.html
  frappe.templates["photoswipe_dom"] = `

<!-- http://photoswipe.com/documentation/getting-started.html -->
<!-- Root element of PhotoSwipe. -->
<div class="pswp" tabindex="-1" role="dialog" aria-hidden="true">

	<!-- Background of PhotoSwipe.
		 It's a separate element as animating opacity is faster than rgba(). -->
	<div class="pswp__bg"></div>

	<!-- Slides wrapper with overflow:hidden. -->
	<div class="pswp__scroll-wrap">

		<!-- Container that holds slides.
			PhotoSwipe keeps only 3 of them in the DOM to save memory.
			Do not modify these 3 pswp__item elements, data is added later on. -->
		<div class="pswp__container">
			<div class="pswp__item"></div>
			<div class="pswp__item"></div>
			<div class="pswp__item"></div>
		</div>

		<div class="pswp__more-items">

		</div>

		<!-- Default (PhotoSwipeUI_Default) interface on top of sliding area. Can be changed. -->
		<div class="pswp__ui pswp__ui--hidden">

			<div class="pswp__top-bar">

				<!--  Controls are self-explanatory. Order can be changed. -->

				<div class="pswp__counter"></div>

				<button class="pswp__button pswp__button--close" title="Close (Esc)"></button>

				<button class="pswp__button pswp__button--share" title="Share"></button>

				<button class="pswp__button pswp__button--fs" title="Toggle fullscreen"></button>

				<button class="pswp__button pswp__button--zoom" title="Zoom in/out"></button>

				<!-- Preloader demo http://codepen.io/dimsemenov/pen/yyBWoR -->
				<!-- element will get class pswp__preloader--active when preloader is running -->
				<div class="pswp__preloader">
					<div class="pswp__preloader__icn">
					  <div class="pswp__preloader__cut">
						<div class="pswp__preloader__donut"></div>
					  </div>
					</div>
				</div>
			</div>

			<div class="pswp__share-modal pswp__share-modal--hidden pswp__single-tap">
				<div class="pswp__share-tooltip"></div>
			</div>

			<button class="pswp__button pswp__button--arrow--left" title="Previous (arrow left)">
			</button>

			<button class="pswp__button pswp__button--arrow--right" title="Next (arrow right)">
			</button>

			<div class="pswp__caption">
				<div class="pswp__caption__center"></div>
			</div>

		</div>

	</div>

</div>
`;

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/views/kanban/kanban_board.html
  frappe.templates["kanban_board"] = `<div class="kanban">
	<div class="kanban-column add-new-column">
		<div class="kanban-column-title compose-column">
			<a> + {{ __("Add Column") }}</a>
		</div>
		<form class="compose-column-form kanban-column-title">
			<input class="new-column-title" name="title" type="text" autocomplete="off">
		</form>
	</div>
	<div class="kanban-empty-state text-muted text-center" style="display: none;">
		{{ __("Loading...") }}
	</div>
</div>`;

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/views/kanban/kanban_column.html
  frappe.templates["kanban_column"] = `<div class="kanban-column" data-column-value="{{title}}" style="background-color: var(--bg-{{indicator}});">
	<div class="kanban-column-header">
		<span class="kanban-column-title">
			<span class="indicator-pill {{indicator}}"></span>
			<span class="kanban-title ellipsis" title="{{title}}">{{ __(title) }}</span>
		</span>
		<div class="column-options dropdown pull-right">
			<a data-toggle="dropdown" aria-haspopup="true" aria-expanded="false">
				<svg class="icon icon-sm">
					<use href="#icon-dot-horizontal"></use>
				</svg>
			</a>
			<ul class="dropdown-menu" style="max-height: 300px; overflow-y: auto;">
				<li><a class="dropdown-item" data-action="archive">{{ __("Archive") }}</a></li>
			</ul>
		</div>
	</div>
	<div class="add-card">
		<div class="ellipsis">
			+ {{ __("Add {0}", [__(doctype)]) }}
		</div>
	</div>
	<div class="kanban-card new-card-area">
		<textarea name="title"></textarea>
	</div>
	<div class="kanban-cards">
	</div>
</div>`;

  // frappe-html:/Users/johannnefdt/RustroverProjects/open_frappe/apps/frappe/frappe/public/js/frappe/views/kanban/kanban_card.html
  frappe.templates["kanban_card"] = `<div class="kanban-card-wrapper {{ disable_click }}" data-name="{{encodeURIComponent(name)}}">
	<div class="kanban-card content">
		{% if(image_url) { %}
		<div class="kanban-image">
			<img  src="{{image_url}}" alt="{{title}}">
		</div>
		{% } %}
		<div class="kanban-card-body">
			<div class="kanban-title-area">
				<a href="{{ form_link }}">
					<div class="kanban-card-title ellipsis" title="{{title}}">
						{{ title }}
					</div>
				</a>
				<br>
				<div class="kanban-card-doc text-muted">
					{{ doc_content }}
				</div>
			</div>
			<div class="kanban-card-meta">
			</div>
		</div>
	</div>
</div>
`;
})();
//# sourceMappingURL=list.bundle.SHXGAR4G.js.map
